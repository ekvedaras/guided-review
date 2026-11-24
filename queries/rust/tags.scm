; Definitions. A function in an impl or trait is also matched as a function; the method wins.
(struct_item name: (type_identifier) @name) @definition.struct
(union_item name: (type_identifier) @name) @definition.struct
(enum_item name: (type_identifier) @name) @definition.enum
(trait_item name: (type_identifier) @name) @definition.trait
(type_item name: (type_identifier) @name) @definition.type
(function_item name: (identifier) @name) @definition.function
(impl_item body: (declaration_list (function_item name: (identifier) @name) @definition.method))
(trait_item body: (declaration_list (function_item name: (identifier) @name) @definition.method))
(trait_item body: (declaration_list (function_signature_item name: (identifier) @name) @definition.method))
(const_item name: (identifier) @name) @definition.constant
(static_item name: (identifier) @name) @definition.constant
(mod_item name: (identifier) @name) @definition.module
(macro_definition name: (identifier) @name) @definition.macro

; References: every name. Which word they name is decided by their text.
(identifier) @name @reference.name
(type_identifier) @name @reference.name
(field_identifier) @name @reference.name
