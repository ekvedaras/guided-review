; Definitions. A struct or interface type is also matched as a type; the struct or interface wins.
(function_declaration name: (identifier) @name) @definition.function
(method_declaration name: (field_identifier) @name) @definition.method
(method_elem name: (field_identifier) @name) @definition.method
(type_spec name: (type_identifier) @name type: (struct_type)) @definition.struct
(type_spec name: (type_identifier) @name type: (interface_type)) @definition.interface
(type_spec name: (type_identifier) @name) @definition.type
(type_alias name: (type_identifier) @name) @definition.type
(source_file (const_declaration (const_spec name: (identifier) @name) @definition.constant))
(source_file (var_declaration (var_spec name: (identifier) @name) @definition.constant))
(source_file (var_declaration (var_spec_list (var_spec name: (identifier) @name) @definition.constant)))

; References: every name. Which word they name is decided by their text.
(identifier) @name @reference.name
(type_identifier) @name @reference.name
(field_identifier) @name @reference.name
