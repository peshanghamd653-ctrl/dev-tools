;; SEC-102. A plugin that spends none of its own budget and all of the host's.
;;
;; `log` needs no permission at all — `Capability::Ambient` — so this module
;; loads under an empty manifest. The loop body is three guest instructions,
;; which is all fuel used to charge for it; the host call it makes copies up to
;; 64 KiB out of guest memory and appends it to a journal that was never
;; drained. Measured before the fix: 1,666,631 calls, 101.7 GiB read, 52
;; seconds, on a budget that was supposed to stop a runaway "in milliseconds".
;;
;; Two exports, because the two failures are different. `flood_big` is about
;; volume — the bytes the host is made to copy. `flood_small` is about count —
;; the entries the host is made to retain. A fix for either alone leaves the
;; other.
(module
  (import "devos" "log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "flood")

  ;; 64 KiB per call: exactly the per-call read cap, which is per call and
  ;; therefore no cap at all on its own.
  (func (export "flood_big")
    (loop $again
      (call $log (i32.const 0) (i32.const 65536))
      (br $again)))

  ;; Five bytes per call: too small for a byte budget to notice, so the entry
  ;; count is the only thing standing between this and an unbounded `Vec`.
  (func (export "flood_small")
    (loop $again
      (call $log (i32.const 0) (i32.const 5))
      (br $again))))
