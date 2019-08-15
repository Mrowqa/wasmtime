#include <setjmp.h>
#include <stdio.h>

#include "SignalHandlers.hpp"

#if defined(_WIN32) && 0

// #include <exception>
// class UnwindException : public std::exception {};

#include <windows.h>
#include <malloc.h>
// extern "C"
int WasmtimeCallTrampoline(void *vmctx, void (*body)(void*, void*), void *args) {
  printf("aaaaaaaaaaaaaaaaaaaa\n");
  __try {
  // try {
    body(vmctx, args);
  }
  __except(/*WasmTrapHandlerFilter(GetExceptionInformation())*/EXCEPTION_EXECUTE_HANDLER) { // todo use some filter
  // catch (UnwindException &ex) {
    printf("unwound\n"); fflush(stdout);
    // if (GetExceptionCode() == EXCEPTION_STACK_OVERFLOW) {
    //   _resetstkoflw();
    // }
    return 0;
  }
  // body(vmctx, args);
  return 1;
}

// extern "C"
int WasmtimeCall(void *vmctx, void (*body)(void*)) {
  abort();
  // jmp_buf buf;
  // void *volatile prev;
  // printf("bbbbbbbbbbbbbbbbbbb\n");
  // if (setjmp(buf) != 0) {
  //   printf("unwound\n"); fflush(stdout);
  //   LeaveScope(prev);
  //   return 0;
  // }
  // prev = EnterScope(&buf); // todo before setjmp?
  // body(vmctx);
  // LeaveScope(prev);
  // return 1;
}

// extern "C"
void Unwind() {
  printf("unwinding\n"); fflush(stdout);
  //abort();
  // throw UnwindException();
  jmp_buf *buf = (jmp_buf*) GetScope();
  longjmp(*buf, 1);
}

#else // not a Windows

// extern "C"
int WasmtimeCallTrampoline(void *vmctx, void (*body)(void*, void*), void *args) {
  jmp_buf buf;
  void *volatile prev;
  if (setjmp(buf) != 0) {
    printf("unwound\n"); fflush(stdout);
    LeaveScope(prev);
    return 0;
  }
  prev = EnterScope(&buf); // todo before setjmp?
  body(vmctx, args);
  LeaveScope(prev);
  return 1;
}

// extern "C"
int WasmtimeCall(void *vmctx, void (*body)(void*)) {
  jmp_buf buf;
  void *volatile prev;
  if (setjmp(buf) != 0) {
    printf("unwound\n"); fflush(stdout);
    LeaveScope(prev);
    return 0;
  }
  prev = EnterScope(&buf); // todo before setjmp?
  body(vmctx);
  LeaveScope(prev);
  return 1;
}

// extern "C"
void Unwind() {
  printf("unwinding\n"); fflush(stdout);
  jmp_buf *buf = (jmp_buf*) GetScope();
  longjmp(*buf, 1);
}

#endif
