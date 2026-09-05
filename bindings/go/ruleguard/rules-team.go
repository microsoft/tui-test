//go:build ruleguard
// +build ruleguard

package gorules

import "github.com/quasilyte/go-ruleguard/dsl"

// Team rules that upstream tooling and Go culture won't enforce for us.
// Stdlib packages are pre-loaded in ruleguard's import table, so patterns can use:
//   json, http, strconv, context, url, etc.

func receiverNameMinLength(m dsl.Matcher) {
	// Enforce: receiver identifier must not be 1 character.
	isSingleChar := func(v dsl.Var) bool {
		return v.Text.Matches(`^[a-zA-Z]$`) && !v.Text.Matches(`^t$`)
	}

	m.Match(`func ($recv $recvType) $name($*args) $*results { $*_ }`).
		Where(isSingleChar(m["recv"])).
		Report(`receiver name must be a meaningful, domain-compliant name (min 2 characters); avoid single-letter receivers`)
}

func forbidIgnoringJSONDecodeError(m dsl.Matcher) {
	// North Star: loud failures.
	// Disallow: `_ = json.NewDecoder(...).Decode(...)`.
	isBlankIdent := func(v dsl.Var) bool {
		return v.Text.Matches(`^_$`)
	}

	m.Import(`encoding/json`)
	m.Match(`$err = json.NewDecoder($r).Decode($v)`).
		Where(isBlankIdent(m["err"])).
		Report(`must check json.Decode error and return/propagate it (do not treat malformed JSON as success)`)
}

func forbidIgnoringHTTPRequestBuildError(m dsl.Matcher) {
	// North Star: loud failures + boundary validation.
	// Disallow: `req, _ := http.NewRequestWithContext(...)`.
	isBlankIdent := func(v dsl.Var) bool {
		return v.Text.Matches(`^_$`)
	}

	m.Import(`net/http`)
	m.Match(`$req, $err = http.NewRequestWithContext($ctx, $method, $url, $body)`).
		Where(isBlankIdent(m["err"])).
		Report(`must check http.NewRequestWithContext error and return/propagate it (invalid URL/context must fail loudly)`)
	m.Match(`$req, $err := http.NewRequestWithContext($ctx, $method, $url, $body)`).
		Where(isBlankIdent(m["err"])).
		Report(`must check http.NewRequestWithContext error and return/propagate it (invalid URL/context must fail loudly)`)
}

func forbidIgnoringAtoiErrorInBoundaryParsing(m dsl.Matcher) {
	// North Star: boundary validation.
	// Disallow: `n, _ := strconv.Atoi(x)`.
	isBlankIdent := func(v dsl.Var) bool {
		return v.Text.Matches(`^_$`)
	}

	m.Import(`strconv`)
	m.Match(`$value, $err = strconv.Atoi($arg)`).
		Where(isBlankIdent(m["err"])).
		Report(`must check strconv.Atoi error and treat invalid input as invalid (no silent coercion)`)
	m.Match(`$value, $err := strconv.Atoi($arg)`).
		Where(isBlankIdent(m["err"])).
		Report(`must check strconv.Atoi error and treat invalid input as invalid (no silent coercion)`)
}
