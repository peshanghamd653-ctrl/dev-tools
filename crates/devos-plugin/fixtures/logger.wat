;; A plugin that passes a string to the host by (pointer, length) into its
;; own linear memory. Proves the host can read guest memory through a
;; bounds-checked accessor, and that an out-of-range (ptr, len) is an error
;; rather than a host-side panic or an out-of-bounds read.
(module
  (import "devos" "log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "hello from plugin")
  (func (export "run")
    (call $log (i32.const 0) (i32.const 17)))
  (func (export "run_out_of_bounds")
    ;; length runs past the end of the single 64 KiB page
    (call $log (i32.const 65000) (i32.const 4096))))
