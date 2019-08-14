#include <setjmp.h>
#include <stdio.h>

#include "SignalHandlers.hpp"

extern "C"
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

extern "C"
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

extern "C"
void Unwind() {
  printf("unwinding\n"); fflush(stdout);
  jmp_buf *buf = (jmp_buf*) GetScope();
  longjmp(*buf, 1);
}
