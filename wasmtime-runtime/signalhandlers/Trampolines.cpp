#include <setjmp.h>

#include "SignalHandlers.hpp"

extern "C"
int WasmtimeCallTrampoline(void *vmctx, void (*body)(void*, void*), void *args) {
  jmp_buf buf;
  void *volatile prev;
  prev = EnterScope(&buf);
  if (setjmp(buf) != 0) {
    LeaveScope(prev);
    return 0;
  }
  body(vmctx, args);
  LeaveScope(prev);
  return 1;
}

extern "C"
int WasmtimeCall(void *vmctx, void (*body)(void*)) {
  jmp_buf buf;
  void *volatile prev;
  prev = EnterScope(&buf);
  if (setjmp(buf) != 0) {
    LeaveScope(prev);
    return 0;
  }
  body(vmctx);
  LeaveScope(prev);
  return 1;
}

extern "C"
void Unwind() {
  jmp_buf *buf = (jmp_buf*) GetScope();
  longjmp(*buf, 1);
}
