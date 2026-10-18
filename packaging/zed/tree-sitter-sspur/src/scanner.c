// String scanning for SSPUR, which follows lex_string and group in
// crates/sspur-syntax/src/lexer.rs. A `{` in a string starts an interpolation
// when its group `{...}` closes on the line and holds an expression that is not
// a lone number (`{3}`), nor a lone string ending where code could go on
// (`"{" or s == "}"`), and does not start with `{` (`{{x}}`). Otherwise the
// brace is text: the `{` alone when the group has the string's quote in it,
// else the whole group. Whether the group is an expression is approximated:
// it must lex, and no two operands may be adjacent.
//
// It also reads the `.`s of `pkg.module.Name` as one name, as the lexer does.

#include "tree_sitter/parser.h"

#include <stdbool.h>
#include <stdint.h>
#include <string.h>

enum TokenType {
  DQ_CONTENT,
  SQ_CONTENT,
  INTERPOLATION_START,
  ERROR_SENTINEL,
  SCOPED_DOT,
};

void *tree_sitter_sspur_external_scanner_create(void) { return NULL; }

void tree_sitter_sspur_external_scanner_destroy(void *payload) { (void)payload; }

unsigned tree_sitter_sspur_external_scanner_serialize(void *payload, char *buffer) {
  (void)payload;
  (void)buffer;
  return 0;
}

void tree_sitter_sspur_external_scanner_deserialize(void *payload, const char *buffer, unsigned length) {
  (void)payload;
  (void)buffer;
  (void)length;
}

static inline void advance(TSLexer *lexer) { lexer->advance(lexer, false); }

static inline bool is_lower(int32_t c) { return (c >= 'a' && c <= 'z') || c == '_'; }
static inline bool is_upper(int32_t c) { return c >= 'A' && c <= 'Z'; }
static inline bool is_digit(int32_t c) { return c >= '0' && c <= '9'; }
static inline bool is_word(int32_t c) { return is_lower(c) || is_upper(c) || is_digit(c); }

#define GROUP_CAP 1024

typedef struct {
  char b[GROUP_CAP];
  unsigned n;
  bool full;
} Group;

static void take(TSLexer *lexer, Group *g) {
  int32_t c = lexer->lookahead;
  if (g->n < GROUP_CAP) g->b[g->n++] = c < 128 ? (char)c : '\x80';
  else g->full = true;
  advance(lexer);
}

static bool read_group(TSLexer *lexer, Group *g);

// After the opening quote of a string inside a group: copy up to its closing quote, and the
// groups in it.
static bool take_string(TSLexer *lexer, Group *g, int32_t quote) {
  while (!lexer->eof(lexer)) {
    int32_t c = lexer->lookahead;
    if (c == '\n') return false;
    take(lexer, g);
    if (c == '\\') {
      if (lexer->eof(lexer) || lexer->lookahead == '\n') return false;
      take(lexer, g);
    } else if (c == quote) {
      return true;
    } else if (c == '{') {
      if (!read_group(lexer, g)) return false;
      take(lexer, g);
    }
  }
  return false;
}

// After a `{` in a string: copy the group up to its matching `}` (not taken), as interp_end does.
static bool read_group(TSLexer *lexer, Group *g) {
  int depth = 1;
  while (!lexer->eof(lexer)) {
    int32_t c = lexer->lookahead;
    switch (c) {
      case '\n':
        return false;
      case '{':
        depth++;
        take(lexer, g);
        break;
      case '}':
        if (--depth == 0) return true;
        take(lexer, g);
        break;
      case '"':
      case '\'':
        take(lexer, g);
        if (!take_string(lexer, g, c)) return false;
        break;
      case '\\':
        take(lexer, g);
        if (lexer->lookahead == '"') {
          // A string written with escaped quotes: copy to the closing `\"`.
          take(lexer, g);
          for (;;) {
            if (lexer->eof(lexer) || lexer->lookahead == '\n') return false;
            int32_t d = lexer->lookahead;
            take(lexer, g);
            if (d == '\\' && lexer->lookahead == '"') {
              take(lexer, g);
              break;
            }
          }
        }
        break;
      default:
        take(lexer, g);
        break;
    }
  }
  return false;
}

static const char *const KEYWORDS[] = {
  "fn", "type", "test", "effect", "handle", "resume", "match", "if", "then", "else", "do",
  "for", "in", "while", "return", "var", "catch", "raise", "with", "where", "and", "or",
  "not", "is", "pub", "use", "par", "yield", "own", "mut", "static", "unsafe", "asm",
  "extern", "kernel", "migrate", NULL,
};

static long skip_braces(const char *s, unsigned n, unsigned i);

// The end of the string whose quote is at `i`, skipping the groups in it, or -1.
static long skip_str(const char *s, unsigned n, unsigned i) {
  char q = s[i];
  for (unsigned j = i + 1; j < n; j++) {
    if (s[j] == '\\') j++;
    else if (s[j] == q) return (long)j;
    else if (s[j] == '{') {
      long e = skip_braces(s, n, j);
      if (e >= 0) j = (unsigned)e;
    }
  }
  return -1;
}

// The matching `}` of the `{` at `i`, or -1.
static long skip_braces(const char *s, unsigned n, unsigned i) {
  int depth = 0;
  for (unsigned j = i; j < n; j++) {
    if (s[j] == '{') depth++;
    else if (s[j] == '}' && --depth == 0) return (long)j;
    else if (s[j] == '"' || s[j] == '\'') {
      long e = skip_str(s, n, j);
      if (e < 0) return -1;
      j = (unsigned)e;
    }
  }
  return -1;
}

static bool keyword(const char *w, unsigned n) {
  for (const char *const *k = KEYWORDS; *k; k++)
    if (strlen(*k) == n && memcmp(*k, w, n) == 0) return true;
  return false;
}

// lexer.rs code_follows: whether `s` can come after a string literal in code.
static bool code_follows(const char *s, unsigned n) {
  unsigned i = 0;
  while (i < n && (s[i] == ' ' || s[i] == '\t')) i++;
  if (i == n) return true; // the group's `}` follows
  unsigned w = i;
  while (w < n && is_word((unsigned char)s[w])) w++;
  if (w > i) {
    static const char *const words[] = {"and", "or", "is", "with", "then", "else", "do", "in", NULL};
    if (is_digit((unsigned char)s[i])) return false;
    for (const char *const *k = words; *k; k++)
      if (strlen(*k) == w - i && memcmp(*k, s + i, w - i) == 0) return true;
    return false;
  }
  return strchr("\r)]},.+=!<>&|;:[", s[i]) != NULL;
}

static const char OPS[] = ".:=!<>+-*/%|;&@";

enum Kind { K_NONE, K_NUM, K_STR, K_NAME, K_KEYWORD, K_OPEN, K_CLOSE, K_MINUS, K_OP, K_OTHER };

// Whether the group `inner` is an expression to interpolate.
static bool interpolates(const char *inner, unsigned len, int32_t quote) {
  char code[GROUP_CAP];
  unsigned n = 0;
  bool esc_q = false, bare_q = false;
  for (unsigned k = 0; k < len; k++) {
    if (inner[k] == '\\' && k + 1 < len && inner[k + 1] == '"') esc_q = true, k++;
    else if (inner[k] == '"') bare_q = true;
  }
  bool escaped_only = esc_q && !bare_q;
  for (unsigned k = 0; k < len; k++) {
    if (escaped_only && inner[k] == '\\' && k + 1 < len && inner[k + 1] == '"') continue;
    code[n++] = inner[k];
  }
  unsigned lead = 0;
  while (lead < n && (code[lead] == ' ' || code[lead] == '\t' || code[lead] == '\r')) lead++;
  if (lead == n || code[lead] == '{') return false;

  enum Kind kinds[3] = {K_NONE, K_NONE, K_NONE};
  enum Kind prev = K_NONE, first = K_NONE;
  unsigned count = 0;
  int depth = 0;
  for (unsigned i = lead; i < n;) {
    unsigned char c = (unsigned char)code[i];
    enum Kind k;
    if (c == ' ' || c == '\t' || c == '\r') { i++; continue; }
    if (c == '/' && i + 1 < n && code[i + 1] == '/') break;
    if (c == '"' || c == '\'') {
      long j = skip_str(code, n, i);
      if (j < 0) return false;
      i = (unsigned)j + 1;
      k = K_STR;
    } else if (is_digit(c)) {
      while (i < n && (is_word((unsigned char)code[i]) || (code[i] == '.' && i + 1 < n && is_digit((unsigned char)code[i + 1])))) i++;
      k = K_NUM;
    } else if (is_word(c)) {
      unsigned s = i;
      while (i < n && is_word((unsigned char)code[i])) i++;
      k = keyword(code + s, i - s) ? K_KEYWORD : K_NAME;
    } else if (c == '(' || c == '[' || c == '{') {
      i++;
      depth++;
      k = K_OPEN;
    } else if (c == ')' || c == ']' || c == '}') {
      i++;
      if (--depth < 0) return false;
      k = K_CLOSE;
    } else if (c == '-' && !(i + 1 < n && code[i + 1] == '>')) {
      i++;
      k = K_MINUS;
    } else if (c == '?') {
      i++;
      while (i < n && is_word((unsigned char)code[i])) i++;
      k = K_NAME;
    } else if (c == ',') {
      if (depth == 0) return false;
      i++;
      k = K_OP;
    } else if (strchr(OPS, c)) {
      unsigned s = i;
      while (i < n && code[i] && strchr(OPS, code[i])) i++;
      // `a: 1`, `a = 1` and `a := 1` are not expressions
      if (depth == 0 && ((i - s == 1 && (c == ':' || c == '=')) || (i - s == 2 && c == ':' && code[s + 1] == '='))) return false;
      k = K_OP;
    } else {
      return false; // not a token: a stray backslash, `$`, `~`, non-ASCII text
    }
    bool ends = prev == K_NUM || prev == K_STR || prev == K_NAME || prev == K_CLOSE;
    bool starts = k == K_NUM || k == K_STR || k == K_NAME;
    if (ends && starts) return false;
    if (count == 0) first = k;
    if (count < 3) kinds[count] = k;
    count++;
    prev = k;
  }
  if (depth != 0 || first == K_OP || first == K_CLOSE) return false;
  if (prev == K_OP || prev == K_MINUS || prev == K_OPEN) return false;
  if (count == 1 && kinds[0] == K_NUM) return false;
  if (count == 2 && kinds[0] == K_MINUS && kinds[1] == K_NUM) return false;
  if (count == 1 && kinds[0] == K_STR) {
    unsigned q = 0;
    while (q < len && (inner[q] == ' ' || inner[q] == '\t')) q++;
    if (code_follows(inner + q + 1, len > q + 1 ? len - q - 1 : 0)) return false;
  }
  (void)quote;
  return true;
}

static bool has_quote(const char *inner, unsigned len, int32_t quote) {
  for (unsigned k = 0; k < len; k++)
    if (inner[k] == quote && (k == 0 || inner[k - 1] != '\\')) return true;
  return false;
}

// At a `.` after a name: whether lowercase names and then a type follow, as in `pkg.module.Name`.
static bool scoped_dot(TSLexer *lexer) {
  advance(lexer);
  lexer->mark_end(lexer);
  for (;;) {
    if (is_upper(lexer->lookahead)) return true;
    if (!is_lower(lexer->lookahead)) return false;
    while (is_word(lexer->lookahead)) advance(lexer);
    if (lexer->lookahead != '.') return false;
    advance(lexer);
  }
}

bool tree_sitter_sspur_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid_symbols) {
  (void)payload;
  if (valid_symbols[ERROR_SENTINEL]) return false;
  bool dq = valid_symbols[DQ_CONTENT];
  bool sq = valid_symbols[SQ_CONTENT];
  if (!dq && !sq) {
    if (valid_symbols[SCOPED_DOT] && lexer->lookahead == '.' && scoped_dot(lexer)) {
      lexer->result_symbol = SCOPED_DOT;
      return true;
    }
    return false;
  }
  int32_t quote = dq ? '"' : '\'';
  TSSymbol content = dq ? DQ_CONTENT : SQ_CONTENT;
  bool any = false;

  for (;;) {
    lexer->mark_end(lexer);
    if (lexer->eof(lexer)) break;
    int32_t c = lexer->lookahead;
    if (c == quote || c == '\\' || c == '\n') break;
    if (c == '{') {
      // The token ends before the `{`; the next call decides what it is.
      if (any) break;
      advance(lexer);
      lexer->mark_end(lexer);
      lexer->result_symbol = content;
      Group g = {.n = 0, .full = false};
      if (!read_group(lexer, &g)) return true; // a `{` that does not close is text
      if (g.full || interpolates(g.b, g.n, quote)) {
        if (valid_symbols[INTERPOLATION_START]) lexer->result_symbol = INTERPOLATION_START;
        return true;
      }
      if (has_quote(g.b, g.n, quote)) return true; // just the `{`
      advance(lexer);                             // the whole group, with its `}`
      lexer->mark_end(lexer);
      return true;
    }
    advance(lexer);
    any = true;
  }

  if (!any) return false;
  lexer->result_symbol = content;
  return true;
}
