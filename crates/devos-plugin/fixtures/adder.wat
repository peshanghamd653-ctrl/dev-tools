;; Baseline: a plugin that computes something and returns it.
;; Proves module load, typed export lookup, call, and return value.
(module
  (func (export "add") (param i32 i32) (result i32)
    local.get 0
    local.get 1
    i32.add))
