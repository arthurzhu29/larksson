
file: (NEWLINE statement)* [NEWLINE]
statement: set_statement | print_statement
set_statement: var '=' value
print_statement: 'print' var

var: ('.' value)* '.'
value: NUMBER | closed_var | list
closed_var: '(' var ')'
list:
    | '{' ','.list_item+ ','? '}'
    | '{' '}'
list_item: value ':' value
