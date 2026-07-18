// Signature qualification for the ts2rust fuzz harness (F3 rung 1).
// A candidate is only drivable when every param and the return type fit the
// harness value model: string/number/boolean scalars, string[]/number[], or a
// flat object literal of those; scalar-ish return (| null / | undefined fine).
// Anything richer (generics, class types, unions of objects) is rejected here
// so the purity pass never wastes time on bodies we cannot fuzz anyway.

const ALIAS_DEPTH_LIMIT = 2 // one alias hop is useful; deeper chains are a smell, not a candidate

export function buildLocalTypeAliases(ts, sourceFile) {
  const aliases = new Map()
  for (const stmt of sourceFile.statements) {
    if (ts.isTypeAliasDeclaration(stmt) && !stmt.typeParameters) {
      aliases.set(stmt.name.text, stmt.type)
    }
  }
  return aliases
}

function unwrapParens(ts, t) {
  while (ts.isParenthesizedTypeNode(t)) {
    t = t.type
  }
  return t
}

// 'string' | 'number' | 'boolean' | null for a single scalar-ish type node.
function scalarKindOf(ts, typeNode) {
  const t = unwrapParens(ts, typeNode)
  const k = ts.SyntaxKind
  if (t.kind === k.StringKeyword) {
    return 'string'
  }
  if (t.kind === k.NumberKeyword) {
    return 'number'
  }
  if (t.kind === k.BooleanKeyword) {
    return 'boolean'
  }
  if (t.kind === k.TemplateLiteralType) {
    return 'string'
  }
  if (ts.isLiteralTypeNode(t)) {
    const lit = t.literal
    if (lit.kind === k.StringLiteral || lit.kind === k.NoSubstitutionTemplateLiteral) {
      return 'string'
    }
    if (lit.kind === k.NumericLiteral || lit.kind === k.PrefixUnaryExpression) {
      return 'number'
    }
    if (lit.kind === k.TrueKeyword || lit.kind === k.FalseKeyword) {
      return 'boolean'
    }
  }
  return null
}

function isNullish(ts, typeNode) {
  const t = unwrapParens(ts, typeNode)
  const k = ts.SyntaxKind
  if (t.kind === k.UndefinedKeyword) {
    return true
  }
  return ts.isLiteralTypeNode(t) && t.literal.kind === k.NullKeyword
}

// Scalar possibly widened by literal-union members and | null / | undefined.
// Returns { base, nullable } or null.
function scalarUnionOf(ts, typeNode, aliases, depth) {
  const t = resolveAlias(ts, unwrapParens(ts, typeNode), aliases, depth)
  if (!t) {
    return null
  }
  if (!ts.isUnionTypeNode(t)) {
    const base = scalarKindOf(ts, t)
    return base ? { base, nullable: false } : null
  }
  let base = null
  let nullable = false
  for (const member of t.types) {
    if (isNullish(ts, member)) {
      nullable = true
      continue
    }
    const kind = scalarKindOf(ts, member)
    if (!kind) {
      return null
    }
    if (base && base !== kind) {
      return null
    } // mixed-kind unions are not harness-drivable
    base = kind
  }
  return base ? { base, nullable } : null
}

function resolveAlias(ts, typeNode, aliases, depth) {
  let t = typeNode
  let hops = 0
  while (
    ts.isTypeReferenceNode(t) &&
    !t.typeArguments &&
    ts.isIdentifier(t.typeName) &&
    aliases.has(t.typeName.text)
  ) {
    if (++hops + depth > ALIAS_DEPTH_LIMIT) {
      return null
    }
    t = unwrapParens(ts, aliases.get(t.typeName.text))
  }
  return t
}

// string[] / number[] (also readonly T[] and Array<T>/ReadonlyArray<T> spellings).
function scalarArrayOf(ts, typeNode, aliases, depth) {
  let t = resolveAlias(ts, unwrapParens(ts, typeNode), aliases, depth)
  if (!t) {
    return null
  }
  if (ts.isTypeOperatorNode(t) && t.operator === ts.SyntaxKind.ReadonlyKeyword) {
    t = t.type
  }
  let element = null
  if (ts.isArrayTypeNode(t)) {
    element = t.elementType
  } else if (
    ts.isTypeReferenceNode(t) &&
    ts.isIdentifier(t.typeName) &&
    (t.typeName.text === 'Array' || t.typeName.text === 'ReadonlyArray') &&
    t.typeArguments?.length === 1
  ) {
    element = t.typeArguments[0]
  }
  if (!element) {
    return null
  }
  const scalar = scalarUnionOf(ts, element, aliases, depth)
  if (!scalar || scalar.nullable || scalar.base === 'boolean') {
    return null
  } // harness drives string[]/number[] only
  return `${scalar.base}[]`
}

function flatObjectSpecOf(ts, typeNode, aliases, depth) {
  const t = resolveAlias(ts, unwrapParens(ts, typeNode), aliases, depth)
  if (!t || !ts.isTypeLiteralNode(t)) {
    return null
  }
  const parts = []
  for (const member of t.members) {
    if (!ts.isPropertySignature(member) || !member.type) {
      return null
    }
    const name =
      ts.isIdentifier(member.name) || ts.isStringLiteral(member.name) ? member.name.text : null
    if (!name) {
      return null
    }
    const scalar = scalarUnionOf(ts, member.type, aliases, depth + 1)
    const arr = scalar ? null : scalarArrayOf(ts, member.type, aliases, depth + 1)
    if (!scalar && !arr) {
      return null
    }
    const spec = scalar ? scalar.base + (scalar.nullable ? '|null' : '') : arr
    parts.push(`${name}${member.questionToken ? '?' : ''}:${spec}`)
  }
  return parts.length > 0 ? `{${parts.join(', ')}}` : null
}

export function describeParamType(ts, param, aliases) {
  if (param.dotDotDotToken) {
    return { ok: false, reason: 'rest param' }
  }
  const optional = Boolean(param.questionToken || param.initializer)
  if (!param.type) {
    // Untyped param: only a literal default gives us a confident kind.
    const init = param.initializer
    const k = ts.SyntaxKind
    const kind =
      init &&
      (init.kind === k.StringLiteral || init.kind === k.NoSubstitutionTemplateLiteral
        ? 'string'
        : init.kind === k.NumericLiteral
          ? 'number'
          : init.kind === k.TrueKeyword || init.kind === k.FalseKeyword
            ? 'boolean'
            : null)
    if (!kind) {
      return { ok: false, reason: 'untyped param' }
    }
    return { ok: true, spec: `${kind}?` }
  }
  const scalar = scalarUnionOf(ts, param.type, aliases, 0)
  if (scalar) {
    return {
      ok: true,
      spec: scalar.base + (scalar.nullable ? '|null' : '') + (optional ? '?' : '')
    }
  }
  const arr = scalarArrayOf(ts, param.type, aliases, 0)
  if (arr) {
    return { ok: true, spec: arr + (optional ? '?' : '') }
  }
  const obj = flatObjectSpecOf(ts, param.type, aliases, 0)
  if (obj) {
    return { ok: true, spec: obj + (optional ? '?' : '') }
  }
  return { ok: false, reason: 'non-drivable param type' }
}

export function describeReturnType(ts, typeNode, aliases) {
  if (typeNode.kind === ts.SyntaxKind.TypePredicate || ts.isTypePredicateNode?.(typeNode)) {
    return { ok: true, spec: 'boolean' } // `x is T` is a boolean at runtime
  }
  const scalar = scalarUnionOf(ts, typeNode, aliases, 0)
  if (scalar) {
    return { ok: true, spec: scalar.base + (scalar.nullable ? '|null' : '') }
  }
  return { ok: false, reason: 'non-scalar return' }
}

// --- shallow return inference for unannotated functions -----------------------
// We only trust first-order syntactic evidence; anything murky bails to null so
// the function is skipped rather than mis-specced.

const STRING_METHODS = new Set([
  'join',
  'trim',
  'trimStart',
  'trimEnd',
  'substring',
  'toLowerCase',
  'toUpperCase',
  'repeat',
  'padStart',
  'padEnd',
  'replace',
  'replaceAll',
  'charAt',
  'toString',
  'toFixed',
  'normalize',
  'concat'
])
const NUMBER_METHODS = new Set(['indexOf', 'lastIndexOf', 'charCodeAt', 'localeCompare'])
const BOOLEAN_METHODS = new Set([
  'includes',
  'startsWith',
  'endsWith',
  'test',
  'some',
  'every',
  'has'
])
const COMPARISON_OPS = new Set([
  'EqualsEqualsEqualsToken',
  'ExclamationEqualsEqualsToken',
  'EqualsEqualsToken',
  'ExclamationEqualsToken',
  'LessThanToken',
  'GreaterThanToken',
  'LessThanEqualsToken',
  'GreaterThanEqualsToken',
  'InstanceOfKeyword',
  'InKeyword'
])
const ARITHMETIC_OPS = new Set([
  'MinusToken',
  'AsteriskToken',
  'SlashToken',
  'PercentToken',
  'AsteriskAsteriskToken',
  'AmpersandToken',
  'BarToken',
  'CaretToken',
  'LessThanLessThanToken',
  'GreaterThanGreaterThanToken',
  'GreaterThanGreaterThanGreaterThanToken'
])

function classifyReturnExpr(ts, expr, paramKinds) {
  const k = ts.SyntaxKind
  let e = expr
  while (ts.isParenthesizedExpression(e)) {
    e = e.expression
  }
  if (
    e.kind === k.StringLiteral ||
    e.kind === k.NoSubstitutionTemplateLiteral ||
    e.kind === k.TemplateExpression
  ) {
    return 'string'
  }
  if (e.kind === k.NumericLiteral) {
    return 'number'
  }
  if (e.kind === k.TrueKeyword || e.kind === k.FalseKeyword) {
    return 'boolean'
  }
  if (e.kind === k.NullKeyword) {
    return 'null'
  }
  if (ts.isIdentifier(e)) {
    if (e.text === 'undefined') {
      return 'null'
    }
    return paramKinds.get(e.text) ?? null
  }
  if (ts.isPrefixUnaryExpression(e)) {
    if (e.operator === k.ExclamationToken) {
      return 'boolean'
    }
    if (e.operator === k.MinusToken || e.operator === k.PlusToken || e.operator === k.TildeToken) {
      return 'number'
    }
    return null
  }
  if (ts.isBinaryExpression(e)) {
    const op = k[e.operatorToken.kind]
    if (COMPARISON_OPS.has(op)) {
      return 'boolean'
    }
    if (ARITHMETIC_OPS.has(op)) {
      return 'number'
    }
    const left = classifyReturnExpr(ts, e.left, paramKinds)
    const right = classifyReturnExpr(ts, e.right, paramKinds)
    if (op === 'PlusToken') {
      if (left === 'string' || right === 'string') {
        return 'string'
      }
      return left === 'number' && right === 'number' ? 'number' : null
    }
    if (
      op === 'AmpersandAmpersandToken' ||
      op === 'BarBarToken' ||
      op === 'QuestionQuestionToken'
    ) {
      return left === right ? left : null
    }
    return null
  }
  if (ts.isConditionalExpression(e)) {
    const a = classifyReturnExpr(ts, e.whenTrue, paramKinds)
    const b = classifyReturnExpr(ts, e.whenFalse, paramKinds)
    if (a === b) {
      return a
    }
    if (a === 'null' || b === 'null') {
      return a === 'null' ? b : a
    } // nullable base; caller merges
    return null
  }
  if (ts.isCallExpression(e)) {
    const callee = e.expression
    if (ts.isIdentifier(callee)) {
      if (callee.text === 'String') {
        return 'string'
      }
      if (callee.text === 'Number' || callee.text === 'parseInt' || callee.text === 'parseFloat') {
        return 'number'
      }
      if (callee.text === 'Boolean') {
        return 'boolean'
      }
      return null
    }
    if (ts.isPropertyAccessExpression(callee)) {
      const method = callee.name.text
      if (
        ts.isIdentifier(callee.expression) &&
        callee.expression.text === 'JSON' &&
        method === 'stringify'
      ) {
        return 'string'
      }
      if (STRING_METHODS.has(method)) {
        return 'string'
      }
      if (NUMBER_METHODS.has(method)) {
        return 'number'
      }
      if (BOOLEAN_METHODS.has(method)) {
        return 'boolean'
      }
    }
    return null
  }
  if (ts.isPropertyAccessExpression(e) && e.name.text === 'length') {
    return 'number'
  }
  return null
}

export function inferReturnSpec(ts, fn, aliases, paramResults) {
  const paramKinds = new Map()
  for (let i = 0; i < fn.parameters.length; i++) {
    const p = fn.parameters[i]
    const spec = paramResults[i]?.spec
    if (ts.isIdentifier(p.name) && spec) {
      const base = spec.replace(/[?|].*$/, '').replace(/\{.*/, '')
      if (base === 'string' || base === 'number' || base === 'boolean') {
        paramKinds.set(p.name.text, base)
      }
    }
  }
  const returns = []
  // Collect returns belonging to THIS function only — nested callbacks return elsewhere.
  const collect = (node) => {
    if (ts.isFunctionLike(node) || ts.isClassLike(node)) {
      return
    }
    if (node.kind === ts.SyntaxKind.ReturnStatement) {
      if (node.expression) {
        returns.push(node.expression)
      }
      return
    }
    ts.forEachChild(node, collect)
  }
  if (!fn.body) {
    return { ok: false, reason: 'no body' }
  }
  if (ts.isBlock(fn.body)) {
    ts.forEachChild(fn.body, collect)
  } else {
    returns.push(fn.body)
  } // arrow expression body
  if (returns.length === 0) {
    return { ok: false, reason: 'no return statements' }
  }
  let base = null
  let nullable = false
  for (const r of returns) {
    const kind = classifyReturnExpr(ts, r, paramKinds)
    if (!kind) {
      return { ok: false, reason: 'un-inferable return' }
    }
    if (kind === 'null') {
      nullable = true
      continue
    }
    if (base && base !== kind) {
      return { ok: false, reason: 'mixed return kinds' }
    }
    base = kind
  }
  if (!base) {
    return { ok: false, reason: 'only null returns' }
  }
  return { ok: true, spec: `${base + (nullable ? '|null' : '')} (inferred)` }
}
