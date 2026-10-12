(function_definition
  "fn" @context
  name: (identifier) @name) @item

(type_definition
  "type" @context
  name: (type_identifier) @name) @item

(test_definition
  "test" @context
  name: (identifier) @name) @item

(trait_definition
  "trait" @context
  name: (type_identifier) @name) @item

(impl_definition
  "impl" @context
  trait: (_) @name
  "for"? @context
  type: (_)? @name) @item

(effect_definition
  "effect" @context
  name: (identifier) @name) @item

(rule_definition
  "rule" @context
  name: (identifier) @name) @item

(store_definition
  "store" @context
  name: (type_identifier) @name) @item

(service_definition
  "svc" @context
  name: (identifier) @name) @item
