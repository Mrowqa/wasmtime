//! Memory management for executable code.

use core::{cmp, mem};
use region;
use std::boxed::Box;
use std::string::String;
use std::vec::Vec;
use wasmtime_runtime::{Mmap, VMFunctionBody};

/// Memory manager for executable code.
pub(crate) struct CodeMemory {
    current: Mmap,
    mmaps: Vec<Mmap>,
    position: usize,
    published: usize,
}

impl CodeMemory {
    /// Create a new `CodeMemory` instance.
    pub fn new() -> Self {
        Self {
            current: Mmap::new(),
            mmaps: Vec::new(),
            position: 0,
            published: 0,
        }
    }

    /// Allocate `size` bytes of memory which can be made executable later by
    /// calling `publish()`. Note that we allocate the memory as writeable so
    /// that it can be written to and patched, though we make it readonly before
    /// actually executing from it.
    ///
    /// TODO: Add an alignment flag.
    fn allocate(&mut self, size: usize) -> Result<&mut [u8], String> {
        if self.current.len() - self.position < size {
            // For every mapping on Windows, we need an extra information for structured
            // exception handling. We use the same handler for every function, so just
            // one record for single mmap is fine.
                #[cfg(all(target_os = "windows", target_pointer_width = "64"))]
                let size = size + region::page::size();
            self.mmaps.push(mem::replace(
                &mut self.current,
                Mmap::with_at_least(cmp::max(0x10000, size))?,
            ));
            self.position = 0;
                #[cfg(all(target_os = "windows", target_pointer_width = "64"))]
                {
                    host_impl::register_executable_memory(&mut self.current);
                    self.position += region::page::size();
                }
        }
        let old_position = self.position;
        self.position += size;
        Ok(&mut self.current.as_mut_slice()[old_position..self.position])
    }

    /// Convert mut a slice from u8 to VMFunctionBody.
    fn view_as_mut_vmfunc_slice(slice: &mut [u8]) -> &mut [VMFunctionBody] {
        let byte_ptr: *mut [u8] = slice;
        let body_ptr = byte_ptr as *mut [VMFunctionBody];
        unsafe { &mut *body_ptr }
    }

    /// Allocate enough memory to hold a copy of `slice` and copy the data into it.
    /// TODO: Reorganize the code that calls this to emit code directly into the
    /// mmap region rather than into a Vec that we need to copy in.
    pub fn allocate_copy_of_byte_slice(
        &mut self,
        slice: &[u8],
    ) -> Result<&mut [VMFunctionBody], String> {
        let new = self.allocate(slice.len())?;
        new.copy_from_slice(slice);
        Ok(Self::view_as_mut_vmfunc_slice(new))
    }

    /// Allocate enough continuous memory block for multiple code blocks. See also
    /// allocate_copy_of_byte_slice.
    pub fn allocate_copy_of_byte_slices(
        &mut self,
        slices: &[&[u8]],
    ) -> Result<Box<[&mut [VMFunctionBody]]>, String> {
        let total_len = slices.into_iter().fold(0, |acc, slice| acc + slice.len());
        let new = self.allocate(total_len)?;
        let mut tail = new;
        let mut result = Vec::with_capacity(slices.len());
        for slice in slices {
            let (block, next_tail) = tail.split_at_mut(slice.len());
            block.copy_from_slice(slice);
            tail = next_tail;
            result.push(Self::view_as_mut_vmfunc_slice(block));
        }
        Ok(result.into_boxed_slice())
    }

    /// Make all allocated memory executable.
    pub fn publish(&mut self) {
        self.mmaps
            .push(mem::replace(&mut self.current, Mmap::new()));
        self.position = 0;

        for m in &mut self.mmaps[self.published..] {
            if m.len() != 0 {
                unsafe {
                    region::protect(m.as_mut_ptr(), m.len(), region::Protection::ReadExecute)
                }
                .expect("unable to make memory readonly and executable");
            }
        }
        self.published = self.mmaps.len();
    }
}

#[cfg(all(target_os = "windows", target_pointer_width = "64"))]
mod host_impl {
    // Docs:
    //    https://docs.microsoft.com/en-us/cpp/build/exception-handling-x64?view=vs-2019
    // SpiderMonkey impl:
    //    https://searchfox.org/mozilla-central/source/js/src/jit/ProcessExecutableMemory.cpp#139-227
    // CppCon 2018 talk about SEH with good example:
    //    https://www.youtube.com/watch?v=COEv2kq_Ht8
    //    https://github.com/CppCon/CppCon2018/blob/master/Presentations/unwinding_the_stack_exploring_how_cpp_exceptions_work_on_windows/unwinding_the_stack_exploring_how_cpp_exceptions_work_on_windows__james_mcnellis__cppcon_2018.pdf
    // Note:
    //    ARM requires different treatment (not implemented)

    use region;
    use std::convert::TryFrom;
    use std::ptr;
    use wasmtime_runtime::Mmap;
    use winapi::ctypes::c_int;
    use winapi::shared::basetsd::ULONG64;
    use winapi::shared::minwindef::{BYTE, ULONG};
    use winapi::shared::ntdef::FALSE;
    use winapi::um::winnt::RtlAddFunctionTable;
    use winapi::um::winnt::{
        PCONTEXT, PDISPATCHER_CONTEXT, PEXCEPTION_RECORD, RUNTIME_FUNCTION, UNW_FLAG_EHANDLER,
    };
    use winapi::vc::excpt::{EXCEPTION_CONTINUE_SEARCH, ExceptionContinueSearch, EXCEPTION_DISPOSITION};

    #[repr(C)]
    struct ExceptionHandlerRecord {
        runtime_function: RUNTIME_FUNCTION,
        unwind_info: UnwindInfo,
        thunk: [u8; 13],//12],
    }

    // Note: this is a bitfield in WinAPI, so some fields are actually merged below
    #[cfg(not(target_arch = "arm"))]
    #[repr(C)]
    struct UnwindInfo {
        version_and_flags: BYTE,
        size_of_prologue: BYTE,
        count_of_unwind_codes: BYTE,
        frame_register_and_offset: BYTE,
        exception_handler: ULONG,
    }
    #[cfg(not(target_arch = "arm"))]
    static FLAGS_BIT_OFFSET: u8 = 3;

    macro_rules! offsetof {
        ($class:ident, $field:ident) => { unsafe {
            (&(*(ptr::null::<$class>())).$field) as *const _
        } as usize };
    }

    #[cfg(not(target_arch = "arm"))]
    pub fn register_executable_memory(mmap: &mut Mmap) {
        eprintln!(
            "register_executable_memory() for mmap {:?}  --  {:?}",
            mmap.as_ptr(),
            unsafe { mmap.as_ptr().add(mmap.len()) },
        );
        let r = unsafe { (mmap.as_mut_ptr() as *mut ExceptionHandlerRecord).as_mut() }.unwrap();
        r.runtime_function.BeginAddress = u32::try_from(region::page::size()).unwrap();
        r.runtime_function.EndAddress = u32::try_from(mmap.len()).unwrap();
        *unsafe { r.runtime_function.u.UnwindInfoAddress_mut() } =
            u32::try_from(offsetof!(ExceptionHandlerRecord, unwind_info)).unwrap();

        r.unwind_info.version_and_flags = 1; // version
        r.unwind_info.version_and_flags |=
            u8::try_from(UNW_FLAG_EHANDLER << FLAGS_BIT_OFFSET).unwrap(); // flags
        r.unwind_info.size_of_prologue = 0;
        r.unwind_info.count_of_unwind_codes = 0;
        r.unwind_info.frame_register_and_offset = 0;
        r.unwind_info.exception_handler =
            u32::try_from(offsetof!(ExceptionHandlerRecord, thunk)).unwrap();

        // mov imm64, rax
        r.thunk[0] = 0x48;
        r.thunk[1] = 0xb8;
        unsafe {
            ptr::write_unaligned::<usize>(
                &mut r.thunk[2] as *mut _ as *mut usize,
                exception_handler as usize,
            )
        };
        r.thunk[10] = 0x90; //0xCC;
        // jmp rax
        r.thunk[10+1] = 0xff;
        r.thunk[11+1] = 0xe0;

        // println!("--------------- {:?} and {:?}", exception_handler as usize, &exception_handler as *const _ as usize);

        // // todo probably not needed, but call it just in case
        // unsafe {
        //     region::protect(
        //         mmap.as_mut_ptr(),
        //         region::page::size(),
        //         region::Protection::ReadExecute,
        //     )
        // }
        // .expect("unable to make memory readonly and executable");

        let res = unsafe {
            RtlAddFunctionTable(
                mmap.as_mut_ptr() as *mut _,
                1,
                u64::try_from(mmap.as_ptr() as usize).unwrap(),
            )
        };
        if res == FALSE {
            panic!("RtlAddFunctionTable() failed");
        }

        eprintln!(
            "register_executable_memory() END for mmap {:?}",
            mmap.as_ptr()
        );

        // Note: our section needs to have read & execute rights for the thunk,
        //       and publish() will do it before executing the JIT code.
    }

    // This method should NEVER be called, because we're using vectored exception handlers
    // which have higher priority. We can do a couple of things:
    // 1) unreachable!()
    // 2) call WasmTrapHandler
    //    -- if exception originates from JIT code, should catch the exception and unwind
    //       the stack
    // 3) return ExceptionContinueSearch, so OS will continue search for exception handlers
    //    -- imported functions should handle their exceptions, and JIT code doesn't raise
    //       its own exceptions
    extern "C" {
        fn Unwind();
        fn _resetstkoflw() -> c_int;
    }
    unsafe extern "C" fn exception_handler(
        _exception_record: PEXCEPTION_RECORD,
        _establisher_frame: ULONG64,
        _context_record: PCONTEXT,
        _dispatcher_context: PDISPATCHER_CONTEXT,
    ) -> EXCEPTION_DISPOSITION {
        // eprintln!("buh");
        // unreachable!("WasmTrapHandler (vectored exception handler) should have already handled the exception");
        assert_eq!(_resetstkoflw(), 0);
        Unwind(); // tmp
        0 // dumb value
        // ExceptionContinueSearch
    }
}
