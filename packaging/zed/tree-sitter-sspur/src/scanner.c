// String scanning for SSPUR, which follows crates/sspur-syntax/src/lexer.rs:
// a `{` in a string starts an interpolation only when a matching `}` follows
// on the same line and there is something between them, so `"{}"` and
// `"([{"` are literal text.

#include "tree_sitter/parser.h"

#include <stdbool.h>
#include <stdint.h>

enum TokenType {
  DQ_CONTENT,
  SQ_CONTENT,
  INTERPOLATION_START,
  ERROR_SENTINEL,
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

// After the opening quote of a string inside an interpolation: skip past its closing quote.
static bool skip_string(TSLexer *lexer, int32_t quote) {
  while (!lexer->eof(lexer)) {
    int32_t c = lexer->lookahead;
    if (c == '\n') return false;
    advance(lexer);
    if (c == '\\') {
      if (lexer->eof(lexer) || lexer->lookahead == '\n') return false;
      advance(lexer);
    } else if (c == quote) {
      return true;
    }
  }
  return false;
}

// After a `{` in a string: whether a matching `}` closes it on this line, with a non-blank inside.
static bool interpolation_closes(TSLexer *lexer) {
  int depth = 1;
  bool content = false;
  while (!lexer->eof(lexer)) {
    int32_t c = lexer->lookahead;
    switch (c) {
      case '\n':
        return false;
      case '{':
        depth++;
        content = true;
        advance(lexer);
        break;
      case '}':
        depth--;
        if (depth == 0) return content;
        advance(lexer);
        break;
      case '"':
      case '\'':
        advance(lexer);
        if (!skip_string(lexer, c)) return false;
        content = true;
        break;
      case '\\':
        advance(lexer);
        if (lexer->lookahead == '"') {
          // A string written with escaped quotes: skip to the closing `\"`.
          advance(lexer);
          for (;;) {
            if (lexer->eof(lexer) || lexer->lookahead == '\n') return false;
            int32_t d = lexer->lookahead;
            advance(lexer);
            if (d == '\\' && lexer->lookahead == '"') {
              advance(lexer);
              break;
            }
          }
          content = true;
        }
        break;
      default:
        if (c != ' ' && c != '\t' && c != '\r') content = true;
        advance(lexer);
        break;
    }
  }
  return false;
}

bool tree_sitter_sspur_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid_symbols) {
  (void)payload;
  if (valid_symbols[ERROR_SENTINEL]) return false;
  bool dq = valid_symbols[DQ_CONTENT];
  bool sq = valid_symbols[SQ_CONTENT];
  if (!dq && !sq) return false;
  int32_t quote = dq ? '"' : '\'';
  TSSymbol content = dq ? DQ_CONTENT : SQ_CONTENT;
  bool any = false;

  for (;;) {
    lexer->mark_end(lexer);
    if (lexer->eof(lexer)) break;
    int32_t c = lexer->lookahead;
    if (c == quote || c == '\\' || c == '\n') break;
    if (c == '{') {
      advance(lexer);
      if (!any) lexer->mark_end(lexer);
      bool opens = interpolation_closes(lexer);
      if (any) {
        // The token ends before the `{`; the next call decides what it is.
        lexer->result_symbol = content;
        return true;
      }
      if (opens && valid_symbols[INTERPOLATION_START]) {
        lexer->result_symbol = INTERPOLATION_START;
        return true;
      }
      // A literal `{`.
      lexer->result_symbol = content;
      return true;
    }
    advance(lexer);
    any = true;
  }

  if (!any) return false;
  lexer->result_symbol = content;
  return true;
}
