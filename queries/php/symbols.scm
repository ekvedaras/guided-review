(class_declaration name: (name) @name) @class
(interface_declaration name: (name) @name) @interface
(trait_declaration name: (name) @name) @trait
(enum_declaration name: (name) @name) @enum
(function_definition name: (name) @name) @function
(const_declaration (const_element (name) @name)) @const

; Methods of anonymous classes stay inside the named symbol around them.
(class_declaration body: (declaration_list (method_declaration name: (name) @name) @method))
(interface_declaration body: (declaration_list (method_declaration name: (name) @name) @method))
(trait_declaration body: (declaration_list (method_declaration name: (name) @name) @method))
(enum_declaration body: (enum_declaration_list (method_declaration name: (name) @name) @method))
