package main

import (
	"eronom/eval"
	"fmt"
)

func main() {
	ev := eval.NewErmEval()
	script := `
		let count = signal(0);
		const items = [1, 2, 3, 4, 5];
	`
	ev.ParseScriptVars(script)
	
	val, err := ev.Eval("items")
	fmt.Printf("items: %v, err: %v, type: %T\n", val, err, val)
	
	val2, err2 := ev.Eval("count")
	fmt.Printf("count: %v, err: %v, type: %T\n", val2, err2, val2)
}
