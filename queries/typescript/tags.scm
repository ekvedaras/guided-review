; Shared by the TypeScript and TSX grammars.
; Definitions. A top-level arrow function is also matched as a constant; the function wins.
(function_declaration name: (identifier) @name) @definition.function
(generator_function_declaration name: (identifier) @name) @definition.function
(class_declaration name: (type_identifier) @name) @definition.class
(abstract_class_declaration name: (type_identifier) @name) @definition.class
(interface_declaration name: (type_identifier) @name) @definition.interface
(enum_declaration name: (identifier) @name) @definition.enum
(type_alias_declaration name: (type_identifier) @name) @definition.type
(method_definition name: (property_identifier) @name) @definition.method
(abstract_method_signature name: (property_identifier) @name) @definition.method
(method_signature name: (property_identifier) @name) @definition.method
(program
  (lexical_declaration
    (variable_declarator name: (identifier) @name value: [(arrow_function) (function_expression)])) @definition.function)
(program
  (export_statement
    (lexical_declaration
      (variable_declarator name: (identifier) @name value: [(arrow_function) (function_expression)])) @definition.function))
(program
  (lexical_declaration (variable_declarator name: (identifier) @name)) @definition.constant)
(program
  (export_statement
    (lexical_declaration (variable_declarator name: (identifier) @name)) @definition.constant))

; References: every name. Which word they name is decided by their text.
(identifier) @name @reference.name
(type_identifier) @name @reference.name
(property_identifier) @name @reference.name
(shorthand_property_identifier) @name @reference.name
(shorthand_property_identifier_pattern) @name @reference.name
