; Definitions. A function in a class body is also matched as a function; the method wins.
(class_definition name: (identifier) @name) @definition.class
(function_definition name: (identifier) @name) @definition.function
(class_definition body: (block (function_definition name: (identifier) @name) @definition.method))
(class_definition body: (block (decorated_definition definition: (function_definition name: (identifier) @name) @definition.method)))
(module (expression_statement (assignment left: (identifier) @name) @definition.constant))

; References: every name. Which word they name is decided by their text.
(identifier) @name @reference.name
