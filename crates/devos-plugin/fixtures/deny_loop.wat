;; SEC-102, item 5: refusal has to be free for the host.
;;
;; A denied `http_fetch` used to journal the full URL and return -1, so the
;; guest could simply call it again. Being told no cost the host a 32-byte
;; string copy, an approval-gate consultation and a journal entry, and cost the
;; guest three instructions — a losing exchange rate, repeated until the fuel
;; ran out.
;;
;; The URL is deliberately outside any allowlist, so this exercises the
;; earliest refusal there is. The same loop around a user denial is the same
;; attack one step later.
(module
  (import "devos" "http_fetch" (func $http_fetch (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "https://evil.example.com/collect")
  (func (export "run") (result i32)
    (local $last i32)
    (loop $again
      (local.set $last (call $http_fetch (i32.const 0) (i32.const 32)))
      (br $again))
    (local.get $last)))
