;; SEC-101, the other half. Capping a single table's elements is not enough if
;; the module may declare arbitrarily many tables: wasmi's `reference-types`
;; feature is on by default, so multiple tables are legal, and `StoreLimits`
;; defaulted to permitting 10,000 of them.
;;
;; Eight tiny tables. Individually harmless — which is the point. The refusal
;; has to come from the table *count*, not from any one table's size.
(module
  (table 1 funcref)
  (table 1 funcref)
  (table 1 funcref)
  (table 1 funcref)
  (table 1 funcref)
  (table 1 funcref)
  (table 1 funcref)
  (table 1 funcref)
  (memory (export "memory") 1)
  (func (export "run") (result i32)
    (i32.const 0)))
