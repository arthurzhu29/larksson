'c' => i32
[1, 2, 3] => { 0: 1, 1: 2, 2: 3 }
.1.2.3. => [1, 2, 3]
"hello" => ['h', 'e', 'l', 'l', 'o']
hello => "hello"
value = value => [value, value, 0] // only within lines
value <- value => [value, value, 1] // only within lines
lines! value value value => [value, value, value] // top-level