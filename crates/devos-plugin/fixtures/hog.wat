;; A plugin that allocates until the host stops it.
;; Proves the linear-memory ceiling is enforced by the host, not by the
;; guest's own declared maximum (which a hostile guest would simply omit).
(module
  (memory (export "memory") 1)
  (func (export "hog") (result i32)
    (local $pages i32)
    (loop $again
      (local.set $pages (memory.grow (i32.const 1)))
      (br_if $again (i32.ne (local.get $pages) (i32.const -1))))
    (local.get $pages)))
