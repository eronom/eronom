package eval

import (
	"fmt"
	"regexp"
	"strconv"
	"strings"
)

// =============================================================================
// Lightweight SSR Expression Evaluator (replaces goja)
// =============================================================================

// ErmEval is a lightweight expression evaluator for SSR.
// It handles variable lookups, dot access, comparisons, and boolean logic.
type ErmEval struct {
	vars map[string]interface{}
}

func NewErmEval() *ErmEval {
	return &ErmEval{vars: make(map[string]interface{})}
}

func (ev *ErmEval) Set(name string, val interface{}) {
	ev.vars[name] = val
}

func (ev *ErmEval) Clone() *ErmEval {
	newEv := NewErmEval()
	for k, v := range ev.vars {
		newEv.vars[k] = v
	}
	return newEv
}

// ParseScriptVars extracts top-level let/const/var declarations from script content.
func (ev *ErmEval) ParseScriptVars(script string) {
	re := regexp.MustCompile(`(?m)(?:let|const|var)\s+([a-zA-Z_$][a-zA-Z0-9_$]*)\s*=\s*`)
	indices := re.FindAllStringSubmatchIndex(script, -1)
	for _, idx := range indices {
		name := script[idx[2]:idx[3]]
		valueStart := idx[1]
		val, _ := parseJSValue(script, valueStart)
		ev.vars[name] = val
	}
}

// Eval evaluates a JS expression string and returns the result.
func (ev *ErmEval) Eval(expr string) (interface{}, error) {
	expr = strings.TrimSpace(expr)
	if expr == "" {
		return nil, fmt.Errorf("empty expression")
	}
	p := &exprParser{input: expr, pos: 0, ev: ev}
	return p.parseExpr()
}

// EvalBool evaluates an expression and returns its boolean value.
func (ev *ErmEval) EvalBool(expr string) (bool, error) {
	val, err := ev.Eval(expr)
	if err != nil {
		return false, err
	}
	return toBool(val), nil
}

func toBool(v interface{}) bool {
	if v == nil {
		return false
	}
	switch val := v.(type) {
	case bool:
		return val
	case float64:
		return val != 0
	case string:
		return val != ""
	default:
		return true
	}
}

func toFloat(v interface{}) float64 {
	if v == nil {
		return 0
	}
	switch val := v.(type) {
	case float64:
		return val
	case bool:
		if val {
			return 1
		}
		return 0
	case string:
		f, _ := strconv.ParseFloat(val, 64)
		return f
	default:
		return 0
	}
}

func valuesEqual(a, b interface{}) bool {
	if a == nil && b == nil {
		return true
	}
	if a == nil || b == nil {
		return false
	}
	af, aNum := a.(float64)
	bf, bNum := b.(float64)
	if aNum && bNum {
		return af == bf
	}
	ab, aBool := a.(bool)
	bb, bBool := b.(bool)
	if aBool && bBool {
		return ab == bb
	}
	return fmt.Sprintf("%v", a) == fmt.Sprintf("%v", b)
}

func isIdentChar(c byte) bool {
	return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_' || c == '$'
}

func skipWS(s string, pos int) int {
	for pos < len(s) && (s[pos] == ' ' || s[pos] == '\t' || s[pos] == '\n' || s[pos] == '\r') {
		pos++
	}
	return pos
}

func parseJSValue(s string, pos int) (interface{}, int) {
	pos = skipWS(s, pos)
	if pos >= len(s) {
		return nil, pos
	}
	if pos+7 <= len(s) && s[pos:pos+7] == "signal(" {
		pos += 7
		val, newPos := parseJSValue(s, pos)
		pos = newPos
		for pos < len(s) && s[pos] != ')' {
			pos++
		}
		if pos < len(s) {
			pos++
		}
		return val, pos
	}
	if s[pos] == '{' {
		return parseJSObject(s, pos)
	}
	if s[pos] == '[' {
		return parseJSArray(s, pos)
	}
	if s[pos] == '"' || s[pos] == '\'' || s[pos] == '`' {
		return parseJSStringLit(s, pos)
	}
	if (s[pos] >= '0' && s[pos] <= '9') || (s[pos] == '-' && pos+1 < len(s) && s[pos+1] >= '0' && s[pos+1] <= '9') {
		return parseJSNumberLit(s, pos)
	}
	if pos+4 <= len(s) && s[pos:pos+4] == "true" && (pos+4 >= len(s) || !isIdentChar(s[pos+4])) {
		return true, pos + 4
	}
	if pos+5 <= len(s) && s[pos:pos+5] == "false" && (pos+5 >= len(s) || !isIdentChar(s[pos+5])) {
		return false, pos + 5
	}
	if pos+4 <= len(s) && s[pos:pos+4] == "null" && (pos+4 >= len(s) || !isIdentChar(s[pos+4])) {
		return nil, pos + 4
	}
	if pos+9 <= len(s) && s[pos:pos+9] == "undefined" && (pos+9 >= len(s) || !isIdentChar(s[pos+9])) {
		return nil, pos + 9
	}
	return nil, pos
}

func parseJSObject(s string, pos int) (map[string]interface{}, int) {
	obj := make(map[string]interface{})
	pos++
	for {
		pos = skipWS(s, pos)
		if pos >= len(s) || s[pos] == '}' {
			if pos < len(s) {
				pos++
			}
			break
		}
		var key string
		if s[pos] == '"' || s[pos] == '\'' {
			var k interface{}
			k, pos = parseJSStringLit(s, pos)
			key = fmt.Sprintf("%v", k)
		} else {
			start := pos
			for pos < len(s) && isIdentChar(s[pos]) {
				pos++
			}
			key = s[start:pos]
		}
		pos = skipWS(s, pos)
		if pos < len(s) && s[pos] == ':' {
			pos++
		}
		var val interface{}
		val, pos = parseJSValue(s, pos)
		obj[key] = val
		pos = skipWS(s, pos)
		if pos < len(s) && s[pos] == ',' {
			pos++
		}
	}
	return obj, pos
}

func parseJSArray(s string, pos int) ([]interface{}, int) {
	arr := []interface{}{}
	pos++
	for {
		pos = skipWS(s, pos)
		if pos >= len(s) || s[pos] == ']' {
			if pos < len(s) {
				pos++
			}
			break
		}
		var val interface{}
		val, pos = parseJSValue(s, pos)
		arr = append(arr, val)
		pos = skipWS(s, pos)
		if pos < len(s) && s[pos] == ',' {
			pos++
		}
	}
	return arr, pos
}

func parseJSStringLit(s string, pos int) (string, int) {
	quote := s[pos]
	pos++
	var sb strings.Builder
	for pos < len(s) && s[pos] != quote {
		if s[pos] == '\\' && pos+1 < len(s) {
			pos++
			switch s[pos] {
			case 'n':
				sb.WriteByte('\n')
			case 't':
				sb.WriteByte('\t')
			case '\\':
				sb.WriteByte('\\')
			default:
				sb.WriteByte(s[pos])
			}
		} else {
			sb.WriteByte(s[pos])
		}
		pos++
	}
	if pos < len(s) {
		pos++
	}
	return sb.String(), pos
}

func parseJSNumberLit(s string, pos int) (float64, int) {
	start := pos
	if s[pos] == '-' {
		pos++
	}
	for pos < len(s) && ((s[pos] >= '0' && s[pos] <= '9') || s[pos] == '.') {
		pos++
	}
	f, _ := strconv.ParseFloat(s[start:pos], 64)
	return f, pos
}

// --- Expression parser (recursive descent) ---

type exprParser struct {
	input string
	pos   int
	ev    *ErmEval
}

func (p *exprParser) skip() {
	for p.pos < len(p.input) && (p.input[p.pos] == ' ' || p.input[p.pos] == '\t' || p.input[p.pos] == '\n') {
		p.pos++
	}
}

func (p *exprParser) parseExpr() (interface{}, error) {
	return p.parseOr()
}

func (p *exprParser) parseOr() (interface{}, error) {
	left, err := p.parseAnd()
	if err != nil {
		return nil, err
	}
	for {
		p.skip()
		if p.pos+1 < len(p.input) && p.input[p.pos:p.pos+2] == "||" {
			p.pos += 2
			right, err := p.parseAnd()
			if err != nil {
				return nil, err
			}
			if !toBool(left) {
				left = right
			}
		} else {
			break
		}
	}
	return left, nil
}

func (p *exprParser) parseAnd() (interface{}, error) {
	left, err := p.parseComparison()
	if err != nil {
		return nil, err
	}
	for {
		p.skip()
		if p.pos+1 < len(p.input) && p.input[p.pos:p.pos+2] == "&&" {
			p.pos += 2
			right, err := p.parseComparison()
			if err != nil {
				return nil, err
			}
			if toBool(left) {
				left = right
			}
		} else {
			break
		}
	}
	return left, nil
}

func (p *exprParser) parseComparison() (interface{}, error) {
	left, err := p.parseAddSub()
	if err != nil {
		return nil, err
	}
	p.skip()
	if p.pos >= len(p.input) {
		return left, nil
	}
	rem := p.input[p.pos:]
	op := ""
	if strings.HasPrefix(rem, "===") {
		op = "==="
	} else if strings.HasPrefix(rem, "!==") {
		op = "!=="
	} else if strings.HasPrefix(rem, "==") {
		op = "=="
	} else if strings.HasPrefix(rem, "!=") {
		op = "!="
	} else if strings.HasPrefix(rem, ">=") {
		op = ">="
	} else if strings.HasPrefix(rem, "<=") {
		op = "<="
	} else if strings.HasPrefix(rem, ">") {
		op = ">"
	} else if strings.HasPrefix(rem, "<") {
		op = "<"
	}
	if op == "" {
		return left, nil
	}
	p.pos += len(op)
	right, err := p.parseAddSub()
	if err != nil {
		return nil, err
	}
	lf, rf := toFloat(left), toFloat(right)
	switch op {
	case ">":
		return lf > rf, nil
	case "<":
		return lf < rf, nil
	case ">=":
		return lf >= rf, nil
	case "<=":
		return lf <= rf, nil
	case "==", "===":
		return valuesEqual(left, right), nil
	case "!=", "!==":
		return !valuesEqual(left, right), nil
	}
	return left, nil
}

func (p *exprParser) parseAddSub() (interface{}, error) {
	left, err := p.parseMulDiv()
	if err != nil {
		return nil, err
	}
	for {
		p.skip()
		if p.pos >= len(p.input) {
			break
		}
		c := p.input[p.pos]
		if c == '+' || c == '-' {
			p.pos++
			right, err := p.parseMulDiv()
			if err != nil {
				return nil, err
			}
			if c == '+' {
				_, lStr := left.(string)
				_, rStr := right.(string)
				if lStr || rStr {
					left = fmt.Sprintf("%v", left) + fmt.Sprintf("%v", right)
				} else {
					left = toFloat(left) + toFloat(right)
				}
			} else {
				left = toFloat(left) - toFloat(right)
			}
		} else {
			break
		}
	}
	return left, nil
}

func (p *exprParser) parseMulDiv() (interface{}, error) {
	left, err := p.parseUnary()
	if err != nil {
		return nil, err
	}
	for {
		p.skip()
		if p.pos >= len(p.input) {
			break
		}
		c := p.input[p.pos]
		if c == '*' || c == '/' || c == '%' {
			p.pos++
			right, err := p.parseUnary()
			if err != nil {
				return nil, err
			}
			lf, rf := toFloat(left), toFloat(right)
			switch c {
			case '*':
				left = lf * rf
			case '/':
				if rf == 0 {
					left = 0.0
				} else {
					left = lf / rf
				}
			case '%':
				if rf == 0 {
					left = 0.0
				} else {
					left = float64(int64(lf) % int64(rf))
				}
			}
		} else {
			break
		}
	}
	return left, nil
}

func (p *exprParser) parseUnary() (interface{}, error) {
	p.skip()
	if p.pos < len(p.input) {
		if p.input[p.pos] == '!' {
			p.pos++
			val, err := p.parseUnary()
			if err != nil {
				return nil, err
			}
			return !toBool(val), nil
		}
		if p.input[p.pos] == '-' {
			p.pos++
			val, err := p.parseUnary()
			if err != nil {
				return nil, err
			}
			return -toFloat(val), nil
		}
	}
	return p.parsePrimary()
}

func (p *exprParser) parsePrimary() (interface{}, error) {
	p.skip()
	if p.pos >= len(p.input) {
		return nil, fmt.Errorf("unexpected end of expression")
	}
	c := p.input[p.pos]

	if c == '(' {
		p.pos++
		val, err := p.parseExpr()
		if err != nil {
			return nil, err
		}
		p.skip()
		if p.pos < len(p.input) && p.input[p.pos] == ')' {
			p.pos++
		}
		return val, nil
	}

	if (c >= '0' && c <= '9') || c == '.' {
		start := p.pos
		for p.pos < len(p.input) && ((p.input[p.pos] >= '0' && p.input[p.pos] <= '9') || p.input[p.pos] == '.') {
			p.pos++
		}
		f, _ := strconv.ParseFloat(p.input[start:p.pos], 64)
		return f, nil
	}

	if c == '"' || c == '\'' || c == '`' {
		quote := c
		p.pos++
		var sb strings.Builder
		for p.pos < len(p.input) && p.input[p.pos] != quote {
			if p.input[p.pos] == '\\' && p.pos+1 < len(p.input) {
				p.pos++
				switch p.input[p.pos] {
				case 'n':
					sb.WriteByte('\n')
				case 't':
					sb.WriteByte('\t')
				default:
					sb.WriteByte(p.input[p.pos])
				}
			} else {
				sb.WriteByte(p.input[p.pos])
			}
			p.pos++
		}
		if p.pos < len(p.input) {
			p.pos++
		}
		return sb.String(), nil
	}

	if isIdentChar(c) && !(c >= '0' && c <= '9') {
		start := p.pos
		for p.pos < len(p.input) && isIdentChar(p.input[p.pos]) {
			p.pos++
		}
		name := p.input[start:p.pos]

		switch name {
		case "true":
			return true, nil
		case "false":
			return false, nil
		case "null", "undefined":
			return nil, nil
		}

		val := p.ev.vars[name]
		for p.pos < len(p.input) && p.input[p.pos] == '.' {
			p.pos++
			propStart := p.pos
			for p.pos < len(p.input) && isIdentChar(p.input[p.pos]) {
				p.pos++
			}
			prop := p.input[propStart:p.pos]
			if m, ok := val.(map[string]interface{}); ok {
				val = m[prop]
			} else {
				val = nil
			}
		}
		return val, nil
	}

	return nil, fmt.Errorf("unexpected character: %c", c)
}
