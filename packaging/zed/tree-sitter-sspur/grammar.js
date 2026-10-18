/**
 * Tree-sitter grammar for SSPUR, written for highlighting and outlines.
 *
 * SSPUR is indentation-sensitive, but this grammar doesn't track layout: a
 * file is a flat run of definition headers, tokens and bracketed groups. That
 * keeps it tolerant of unfinished code while still telling apart what an
 * editor needs: definitions and their names, parameters, types,
 * constructors, calls, members, strings with interpolation, and literals.
 */

const SYMBOLS = [
  '...', ':=', '==', '!=', '<=', '>=', '->', '=>', '**', '..', ',', ':', '=',
  '<', '>', '+', '-', '*', '/', '%', '|', '!', '?', ';', '&', '@',
];

// Keywords only where they start a construct (SOFT_KEYWORDS in the lexer); elsewhere
// they are names: `fn why(field: Str, rule: Str)`.
const SOFT = ['rule', 'trait', 'impl', 'store', 'svc', 'effect'];

const KEYWORDS = [
  'var', 'for', 'in', 'if', 'then', 'else', 'match', 'catch', 'handle', 'do',
  'raise', 'return', 'with', 'where', 'and', 'or', 'not', 'par', 'while',
];

module.exports = grammar({
  name: 'sspur',

  word: $ => $.identifier,

  extras: $ => [/\s/, $.comment],

  externals: $ => [
    $._dq_content,
    $._sq_content,
    $._interpolation_start,
    $._error_sentinel,
    $._scoped_dot,
  ],

  rules: {
    source_file: $ => repeat($._element),

    _element: $ => choice(
      $.function_definition,
      $.type_definition,
      $.test_definition,
      $.trait_definition,
      $.impl_definition,
      $.effect_definition,
      $.store_definition,
      $.service_definition,
      $.rule_definition,
      $.member,
      $.paren_group,
      $.bracket_group,
      $.brace_group,
      $.string,
      $.escaped_string,
      $.integer,
      $.float,
      $.boolean,
      $.hole,
      $.hash,
      $.placeholder,
      $.scoped_type_identifier,
      $.identifier,
      $._soft_name,
      $.type_identifier,
      ...KEYWORDS,
      ...SYMBOLS,
    ),

    function_definition: $ => prec.right(seq(
      'fn',
      field('name', $.identifier),
      optional(field('type_parameters', alias($.bracket_group, $.type_parameters))),
      optional(field('parameters', alias($.paren_group, $.parameters))),
    )),

    type_definition: $ => prec.right(seq(
      'type',
      field('name', $.type_identifier),
      optional(field('type_parameters', alias($.bracket_group, $.type_parameters))),
    )),

    test_definition: $ => seq('test', field('name', $.identifier)),

    trait_definition: $ => prec.right(seq(
      'trait',
      field('name', $.type_identifier),
      optional(field('type_parameters', alias($.bracket_group, $.type_parameters))),
    )),

    impl_definition: $ => prec.right(seq(
      'impl',
      optional(field('type_parameters', alias($.bracket_group, $.type_parameters))),
      field('trait', $._type_name),
      optional(field('trait_arguments', alias($.bracket_group, $.type_arguments))),
      optional(seq(
        'for',
        field('type', $._type_name),
        optional(field('type_arguments', alias($.bracket_group, $.type_arguments))),
      )),
    )),

    _soft_name: $ => prec(-1, alias(choice(...SOFT), $.identifier)),

    _type_name: $ => choice($.type_identifier, $.scoped_type_identifier),

    // `pkg.Name` and `pkg.module.Name`, one name in the lexer: a package's (or one of its
    // public modules') type or constructor.
    // The scanner reads a `.` as `_scoped_dot` only when the names after it end in a type.
    scoped_type_identifier: $ => seq(
      field('package', $.identifier),
      repeat(seq(alias($._scoped_dot, '.'), field('module', $.identifier))),
      alias($._scoped_dot, '.'),
      field('name', $.type_identifier),
    ),

    effect_definition: $ => prec.right(seq(
      'effect',
      field('name', $.identifier),
      optional(field('type_parameters', alias($.bracket_group, $.type_parameters))),
      optional(field('parameters', alias($.paren_group, $.parameters))),
    )),

    store_definition: $ => seq('store', field('name', $.type_identifier)),

    service_definition: $ => seq('svc', field('name', $.identifier)),

    rule_definition: $ => prec.right(seq(
      'rule',
      field('name', $.identifier),
      optional(field('parameters', alias($.paren_group, $.parameters))),
    )),

    // `.name`, `.Type`, `.0` and `lib.{f, T}`. A name after `.` is never a keyword.
    member: $ => seq(
      '.',
      field('name', choice($.identifier, $.type_identifier, $.integer, $.float, $.brace_group)),
    ),

    paren_group: $ => seq('(', repeat($._element), ')'),
    bracket_group: $ => seq('[', repeat($._element), ']'),
    brace_group: $ => seq('{', repeat($._element), '}'),

    string: $ => choice(
      seq('"', repeat(choice(alias($._dq_content, $.string_content), $.escape_sequence, $.interpolation)), token.immediate('"')),
      seq('\'', repeat(choice(alias($._sq_content, $.string_content), $.escape_sequence, $.interpolation)), token.immediate('\'')),
    ),

    interpolation: $ => seq(
      alias($._interpolation_start, '{'),
      repeat($._element),
      '}',
    ),

    escape_sequence: _ => token.immediate(choice(/\\u\{[0-9a-fA-F]*\}/, /\\./)),

    // `\"text\"` inside an interpolation is a string written with escaped quotes.
    escaped_string: _ => token(seq('\\"', /[^"\\\n]*/, '\\"')),

    integer: _ => token(choice(
      /0x[0-9a-fA-F_]+/,
      /[0-9][0-9_]*/,
    )),

    float: _ => token(choice(
      /[0-9][0-9_]*\.[0-9][0-9_]*([eE]-?[0-9]+)?/,
      /[0-9][0-9_]*[eE]-?[0-9]+/,
    )),

    boolean: _ => choice('true', 'false'),

    hole: _ => /\?[A-Za-z_][A-Za-z0-9_]*/,

    hash: _ => /#[A-Za-z0-9]*/,

    placeholder: _ => '_',

    identifier: _ => /[a-z_][A-Za-z0-9_]*/,

    type_identifier: _ => /[A-Z][A-Za-z0-9_]*/,

    comment: _ => token(seq('//', /[^\n]*/)),
  },
});
