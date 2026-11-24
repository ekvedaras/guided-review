; Definitions
(class_declaration name: (name) @name) @definition.class
(interface_declaration name: (name) @name) @definition.interface
(trait_declaration name: (name) @name) @definition.trait
(enum_declaration name: (name) @name) @definition.enum
(function_definition name: (name) @name) @definition.function
(method_declaration name: (name) @name) @definition.method
(const_declaration (const_element (name) @name)) @definition.constant

; References: every name. Which word they name is decided by their text.
(name) @name @reference.name
