;; The control for table_hog.wat and many_tables.wat: what a real toolchain
;; emits. One indirect-call table, a handful of entries, one function in it.
;;
;; A limit tight enough to break this would be a limit that breaks every plugin
;; compiled by LLVM or TinyGo, so this fixture is the half of SEC-101's fix
;; that stops the cure being worse than the disease.
(module
  (type $binop (func (param i32 i32) (result i32)))
  (table 4 funcref)
  (elem (i32.const 0) $add)
  (memory (export "memory") 1)
  (func $add (type $binop)
    (i32.add (local.get 0) (local.get 1)))
  (func (export "add_indirect") (param i32 i32) (result i32)
    (call_indirect (type $binop) (local.get 0) (local.get 1) (i32.const 0))))
