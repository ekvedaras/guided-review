; Shared by the TypeScript and TSX grammars.
(function_declaration name: (identifier) @name) @function
(generator_function_declaration name: (identifier) @name) @function
(class_declaration name: (type_identifier) @name) @class
(abstract_class_declaration name: (type_identifier) @name) @class
(interface_declaration name: (type_identifier) @name) @interface
(enum_declaration name: (identifier) @name) @enum
(type_alias_declaration name: (type_identifier) @name) @type_alias

; Private `#members` stay inside their class: a `#` would break piece ids.
(class_declaration body: (class_body (method_definition name: [(property_identifier) (string)] @name) @method))
(abstract_class_declaration body: (class_body (method_definition name: [(property_identifier) (string)] @name) @method))

; Arrow components and functions are only found at the top level of a file.
; Capital first letter = component, anything else = function.
(program
  (lexical_declaration
    (variable_declarator name: (identifier) @name value: [(arrow_function) (function_expression)])) @component
  (#match? @name "^[A-Z]"))
(program
  (export_statement
    (lexical_declaration
      (variable_declarator name: (identifier) @name value: [(arrow_function) (function_expression)])) @component)
  (#match? @name "^[A-Z]"))
(program
  (lexical_declaration
    (variable_declarator name: (identifier) @name value: [(arrow_function) (function_expression)])) @function
  (#match? @name "^[^A-Z]"))
(program
  (export_statement
    (lexical_declaration
      (variable_declarator name: (identifier) @name value: [(arrow_function) (function_expression)])) @function)
  (#match? @name "^[^A-Z]"))

; describe("name", ...), it("name", ...) and test("name", ...) at any depth.
(expression_statement
  (call_expression
    function: (identifier) @_runner
    arguments: (arguments . (string (string_fragment) @name)))
  (#match? @_runner "^(describe|it|test)$")) @test
