;; SEC-101. A plugin that allocates without executing an instruction.
;;
;; A table's declared *minimum* is committed eagerly when the module is
;; instantiated, at 8 bytes an element. Nothing about this module runs, so
;; neither fuel nor the linear-memory ceiling is ever consulted — the host has
;; simply been handed a number and asked to multiply it by 8.
;;
;; 4,194,304 elements is 32 MiB from a file of a couple of hundred bytes: twice
;; the default 16 MiB memory ceiling, from a channel that ceiling does not
;; cover. The number is kept modest so that a regression fails the test rather
;; than the machine; scaling is linear, the store permits several tables, and
;; the reproduced exploit used a 146-byte module to commit 976 MiB.
;;
;; The `run` export exists only so a reviewer can see there is nothing here to
;; execute. If this module instantiates at all, the limit is not working.
(module
  (table 4194304 funcref)
  (memory (export "memory") 1)
  (func (export "run") (result i32)
    (i32.const 0)))
