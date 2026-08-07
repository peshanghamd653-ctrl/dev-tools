;; A plugin that wants the network. It imports `devos.http_fetch` and calls
;; it with a URL held in its own linear memory.
;;
;; This fixture is the interesting one: when the manifest does not grant
;; `net`, the host never defines `devos.http_fetch`, so this module fails to
;; *instantiate* — the call site is unreachable rather than merely refused.
(module
  (import "devos" "http_fetch" (func $http_fetch (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "https://api.acme.com/v1/things")
  (func (export "run") (result i32)
    (call $http_fetch (i32.const 0) (i32.const 30))))
