// Body classification for autoformalize candidates (F3 rung 1).
// Classes, most to least portable:
//   pure-self-contained : only params/locals + whitelisted deterministic globals
//   needs-inline        : calls/reads imported or same-file helpers (recorded for inlining)
//   impure              : touches mutable module-level state / `this`
//   runtime             : clocks, randomness, env, DOM, fs, async — not formalizable
// Rejections are conservative: a false "runtime" costs one candidate, a false
// "pure" costs a bogus corpus entry, so ambiguity always downgrades.

// Globals whose mere use ties the function to a wall clock, env, or I/O.
const RUNTIME_GLOBALS = new Map([
  ['Date', 'date/time'],
  ['Intl', 'locale runtime'],
  ['crypto', 'crypto'],
  ['process', 'process/env'],
  ['window', 'DOM'],
  ['document', 'DOM'],
  ['navigator', 'DOM'],
  ['location', 'DOM'],
  ['history', 'DOM'],
  ['localStorage', 'DOM storage'],
  ['sessionStorage', 'DOM storage'],
  ['fetch', 'network'],
  ['XMLHttpRequest', 'network'],
  ['WebSocket', 'network'],
  ['setTimeout', 'timer'],
  ['setInterval', 'timer'],
  ['clearTimeout', 'timer'],
  ['clearInterval', 'timer'],
  ['queueMicrotask', 'scheduler'],
  ['requestAnimationFrame', 'scheduler'],
  ['performance', 'clock'],
  ['Promise', 'async'],
  ['globalThis', 'global escape'],
  ['console', 'console I/O'],
  ['Buffer', 'node runtime'],
  ['URL', 'URL runtime'],
  ['URLSearchParams', 'URL runtime'],
  ['require', 'dynamic require'],
  ['__dirname', 'fs location'],
  ['__filename', 'fs location'],
  ['Worker', 'worker'],
  ['SharedArrayBuffer', 'worker'],
  ['Atomics', 'worker'],
  ['AbortController', 'async'],
  ['AbortSignal', 'async'],
  ['structuredClone', 'runtime clone'],
  ['alert', 'DOM'],
  ['CustomEvent', 'DOM'],
  ['Blob', 'DOM'],
  ['File', 'DOM']
])

// Deterministic, side-effect-free globals the ts2rust harness can model.
const PURE_GLOBALS = new Set([
  'String',
  'Number',
  'Boolean',
  'Math',
  'JSON',
  'RegExp',
  'Array',
  'Object',
  'Map',
  'Set',
  'Symbol',
  'parseInt',
  'parseFloat',
  'isNaN',
  'isFinite',
  'NaN',
  'Infinity',
  'undefined',
  'encodeURIComponent',
  'decodeURIComponent',
  'encodeURI',
  'decodeURI',
  'Error',
  'TypeError',
  'RangeError',
  'SyntaxError',
  'Uint8Array',
  'Uint16Array',
  'Uint32Array',
  'Int8Array',
  'Int16Array',
  'Int32Array',
  'Float32Array',
  'Float64Array',
  'ArrayBuffer',
  'DataView'
])

const NODE_BUILTINS = new Set([
  'fs',
  'os',
  'path',
  'child_process',
  'crypto',
  'http',
  'https',
  'net',
  'tls',
  'dns',
  'url',
  'util',
  'stream',
  'zlib',
  'events',
  'worker_threads',
  'vm',
  'readline',
  'tty',
  'module',
  'assert',
  'buffer',
  'process',
  'timers'
])

const MUTATING_METHODS = new Set([
  'push',
  'pop',
  'shift',
  'unshift',
  'splice',
  'sort',
  'reverse',
  'fill',
  'copyWithin',
  'set',
  'delete',
  'clear',
  'add'
])

const ASSIGNMENT_OPS = new Set([
  'EqualsToken',
  'PlusEqualsToken',
  'MinusEqualsToken',
  'AsteriskEqualsToken',
  'SlashEqualsToken',
  'PercentEqualsToken',
  'AmpersandEqualsToken',
  'BarEqualsToken',
  'CaretEqualsToken',
  'LessThanLessThanEqualsToken',
  'GreaterThanGreaterThanEqualsToken',
  'GreaterThanGreaterThanGreaterThanEqualsToken',
  'AsteriskAsteriskEqualsToken',
  'BarBarEqualsToken',
  'AmpersandAmpersandEqualsToken',
  'QuestionQuestionEqualsToken'
])

function bareModuleName(spec) {
  const name = spec.startsWith('node:') ? spec.slice(5) : spec
  return name.split('/')[0]
}

function isNodeBuiltinModule(spec) {
  return spec.startsWith('node:') || NODE_BUILTINS.has(bareModuleName(spec))
}

function isPathModule(spec) {
  return bareModuleName(spec) === 'path'
}

export function buildModuleContext(ts, sourceFile) {
  const imports = new Map() // local name -> { from }
  const localFns = new Map() // top-level function name -> node
  const moduleConsts = new Map() // top-level const name -> initializer node | null
  const moduleMutables = new Set() // top-level let/var names
  const collectBindingNames = (name, into) => {
    if (ts.isIdentifier(name)) {
      into(name.text)
    } else if (ts.isObjectBindingPattern(name) || ts.isArrayBindingPattern(name)) {
      for (const el of name.elements) {
        if (ts.isBindingElement(el)) {
          collectBindingNames(el.name, into)
        }
      }
    }
  }
  for (const stmt of sourceFile.statements) {
    if (ts.isImportDeclaration(stmt) && stmt.importClause) {
      const from = stmt.moduleSpecifier.text
      const clause = stmt.importClause
      if (clause.isTypeOnly) {
        continue
      } // type-only imports never execute
      if (clause.name) {
        imports.set(clause.name.text, { from })
      }
      const bindings = clause.namedBindings
      if (bindings && ts.isNamespaceImport(bindings)) {
        imports.set(bindings.name.text, { from, namespace: true })
      }
      if (bindings && ts.isNamedImports(bindings)) {
        for (const el of bindings.elements) {
          if (!el.isTypeOnly) {
            imports.set(el.name.text, { from })
          }
        }
      }
    } else if (ts.isFunctionDeclaration(stmt) && stmt.name) {
      localFns.set(stmt.name.text, stmt)
    } else if (ts.isVariableStatement(stmt)) {
      const isConst = (stmt.declarationList.flags & ts.NodeFlags.Const) !== 0
      for (const decl of stmt.declarationList.declarations) {
        const init = decl.initializer
        if (
          isConst &&
          ts.isIdentifier(decl.name) &&
          init &&
          (ts.isArrowFunction(init) || ts.isFunctionExpression(init))
        ) {
          localFns.set(decl.name.text, init)
        } else {
          collectBindingNames(decl.name, (n) => {
            if (isConst) {
              moduleConsts.set(n, init ?? null)
            } else {
              moduleMutables.add(n)
            }
          })
        }
      }
    } else if (ts.isEnumDeclaration(stmt)) {
      moduleConsts.set(stmt.name.text, stmt) // enum reads are const-table reads
    } else if (ts.isClassDeclaration(stmt) && stmt.name) {
      localFns.set(stmt.name.text, stmt) // class use = same-file dep, never inlineable
    }
  }
  // Module-wide scan: a `const` Map/Set/array/object is still mutable state when
  // any code in the file mutates its contents — readers of it are NOT pure.
  // Scope-blind by design (a shadowing local of the same name also taints):
  // ambiguity always downgrades.
  const mutatedConsts = new Set()
  const noteMutatedRoot = (expr) => {
    const root = rootIdentifierOf(ts, expr)
    if (root && moduleConsts.has(root.text)) {
      mutatedConsts.add(root.text)
    }
  }
  const scanMutations = (node) => {
    if (
      ts.isCallExpression(node) &&
      ts.isPropertyAccessExpression(node.expression) &&
      MUTATING_METHODS.has(node.expression.name.text)
    ) {
      noteMutatedRoot(node.expression.expression)
    } else if (
      ts.isBinaryExpression(node) &&
      ASSIGNMENT_OPS.has(ts.SyntaxKind[node.operatorToken.kind]) &&
      !ts.isIdentifier(node.left)
    ) {
      noteMutatedRoot(node.left)
    } else if (
      (ts.isPrefixUnaryExpression(node) || ts.isPostfixUnaryExpression(node)) &&
      (node.operator === ts.SyntaxKind.PlusPlusToken ||
        node.operator === ts.SyntaxKind.MinusMinusToken)
    ) {
      noteMutatedRoot(node.operand)
    } else if (ts.isDeleteExpression(node)) {
      noteMutatedRoot(node.expression)
    }
    ts.forEachChild(node, scanMutations)
  }
  ts.forEachChild(sourceFile, scanMutations)
  return {
    ts,
    sourceFile,
    imports,
    localFns,
    moduleConsts,
    moduleMutables,
    mutatedConsts,
    helperVerdicts: new Map(),
    collectBindingNames
  }
}

// Shallow check: module consts made only of literals stay in the pure class;
// consts built by calls become same-file deps (their construction must be ported).
function isPureConstInitializer(ts, init) {
  if (!init || !init.kind) {
    return false
  }
  const k = ts.SyntaxKind
  let e = init
  while (
    ts.isParenthesizedExpression?.(e) ||
    e.kind === k.AsExpression ||
    e.kind === k.SatisfiesExpression
  ) {
    e = e.expression ?? e
    if (!e) {
      return false
    }
  }
  if (
    e.kind === k.StringLiteral ||
    e.kind === k.NumericLiteral ||
    e.kind === k.NoSubstitutionTemplateLiteral ||
    e.kind === k.TrueKeyword ||
    e.kind === k.FalseKeyword ||
    e.kind === k.NullKeyword ||
    e.kind === k.RegularExpressionLiteral
  ) {
    return true
  }
  if (ts.isArrayLiteralExpression(e)) {
    return e.elements.every((el) => isPureConstInitializer(ts, el))
  }
  if (ts.isObjectLiteralExpression(e)) {
    return e.properties.every(
      (p) => ts.isPropertyAssignment(p) && isPureConstInitializer(ts, p.initializer)
    )
  }
  if (
    ts.isNewExpression(e) &&
    ts.isIdentifier(e.expression) &&
    (e.expression.text === 'Set' || e.expression.text === 'Map' || e.expression.text === 'RegExp')
  ) {
    return (e.arguments ?? []).every((a) => isPureConstInitializer(ts, a))
  }
  if (ts.isEnumDeclaration?.(e)) {
    return true
  }
  return false
}

function collectFunctionLocals(ts, fn, collectBindingNames) {
  const locals = new Set()
  for (const p of fn.parameters ?? []) {
    collectBindingNames(p.name, (n) => locals.add(n))
  }
  const walk = (node) => {
    if (ts.isVariableDeclaration(node)) {
      collectBindingNames(node.name, (n) => locals.add(n))
    } else if ((ts.isFunctionDeclaration(node) || ts.isClassDeclaration(node)) && node.name) {
      locals.add(node.name.text)
    } else if (ts.isFunctionLike(node)) {
      for (const p of node.parameters ?? []) {
        collectBindingNames(p.name, (n) => locals.add(n))
      }
    } else if (ts.isCatchClause(node) && node.variableDeclaration) {
      collectBindingNames(node.variableDeclaration.name, (n) => locals.add(n))
    }
    ts.forEachChild(node, walk)
  }
  if (fn.body) {
    walk(fn.body)
  }
  return locals
}

function rootIdentifierOf(ts, expr) {
  let e = expr
  while (
    ts.isPropertyAccessExpression(e) ||
    ts.isElementAccessExpression(e) ||
    ts.isNonNullExpression?.(e) ||
    ts.isParenthesizedExpression(e)
  ) {
    e = e.expression
  }
  return ts.isIdentifier(e) ? e : null
}

// True when this identifier node is a value READ (not a declaration, label,
// property name, or type-position mention).
function isValueRead(ts, node, parent) {
  if (!parent) {
    return true
  }
  if (ts.isPropertyAccessExpression(parent) && parent.name === node) {
    return false
  }
  if (ts.isPropertyAssignment(parent) && parent.name === node) {
    return false
  }
  if (ts.isBindingElement(parent) && (parent.name === node || parent.propertyName === node)) {
    return false
  }
  if (ts.isVariableDeclaration(parent) && parent.name === node) {
    return false
  }
  if (ts.isParameter(parent) && parent.name === node) {
    return false
  }
  if (
    ts.isFunctionDeclaration(parent) ||
    ts.isFunctionExpression(parent) ||
    ts.isClassDeclaration(parent)
  ) {
    if (parent.name === node) {
      return false
    }
  }
  if (ts.isMethodDeclaration(parent) && parent.name === node) {
    return false
  }
  if (
    (ts.isLabeledStatement(parent) || ts.isBreakOrContinueStatement?.(parent)) &&
    parent.label === node
  ) {
    return false
  }
  if (ts.isQualifiedName?.(parent) && parent.right === node) {
    return false
  }
  return true
}

export function classifyFunctionBody(ctx, fn, { followLocalHelpers = true } = {}) {
  const { ts } = ctx
  const runtime = new Set()
  const impure = new Set()
  const deps = new Map() // display name -> { name, from, inlineable? }
  const locals = collectFunctionLocals(ctx.ts, fn, ctx.collectBindingNames)

  const addDep = (name, from, inlineable) => {
    const key = `${from}::${name}`
    if (!deps.has(key)) {
      deps.set(key, { name, from, ...(inlineable !== undefined ? { inlineable } : {}) })
    }
  }

  const handleLocalHelperDep = (name) => {
    if (!followLocalHelpers) {
      addDep(name, '(same-file)', false)
      return
    }
    let verdict = ctx.helperVerdicts.get(name)
    if (verdict === undefined) {
      ctx.helperVerdicts.set(name, null) // in-progress marker breaks mutual recursion
      const helper = ctx.localFns.get(name)
      verdict =
        ts.isClassDeclaration?.(helper) || !helper?.body
          ? { cls: 'needs-inline' }
          : classifyFunctionBody(ctx, helper, { followLocalHelpers: false })
      ctx.helperVerdicts.set(name, verdict)
    }
    if (verdict === null) {
      addDep(name, '(same-file)', false) // recursive helper cycle: not depth-1 inlineable
      return
    }
    if (verdict.cls === 'runtime' || verdict.cls === 'impure') {
      // A helper's clock/state use taints the caller: inlining it would import the problem.
      ;(verdict.cls === 'runtime' ? runtime : impure).add(
        `local helper '${name}' is ${verdict.cls} (${verdict.reasons[0] ?? ''})`
      )
      return
    }
    addDep(name, '(same-file)', verdict.cls === 'pure-self-contained')
  }

  const handleIdentifierRead = (node, parent) => {
    const name = node.text
    if (locals.has(name)) {
      return
    }
    if (name === 'arguments') {
      impure.add('uses arguments object')
      return
    }
    const parentProp =
      parent && ts.isPropertyAccessExpression(parent) && parent.expression === node
        ? parent.name.text
        : null
    if (name === 'Math' && parentProp === 'random') {
      runtime.add('Math.random')
      return
    }
    if (RUNTIME_GLOBALS.has(name)) {
      runtime.add(`${name} (${RUNTIME_GLOBALS.get(name)})`)
      return
    }
    if (PURE_GLOBALS.has(name)) {
      return
    }
    const imp = ctx.imports.get(name)
    if (imp) {
      if (isNodeBuiltinModule(imp.from)) {
        if (isPathModule(imp.from)) {
          // path.resolve depends on cwd; the rest of path is portable but must be inlined.
          if (parentProp === 'resolve' || name === 'resolve') {
            runtime.add('path.resolve (cwd-dependent)')
          } else {
            addDep(imp.namespace && parentProp ? `${name}.${parentProp}` : name, imp.from)
          }
        } else {
          runtime.add(`node builtin '${imp.from}'`)
        }
        return
      }
      if (imp.from === 'electron') {
        runtime.add('electron runtime')
        return
      }
      addDep(imp.namespace && parentProp ? `${name}.${parentProp}` : name, imp.from)
      return
    }
    if (ctx.localFns.has(name)) {
      handleLocalHelperDep(name)
      return
    }
    if (ctx.moduleConsts.has(name)) {
      if (ctx.mutatedConsts.has(name)) {
        impure.add(`reads module-level const '${name}' whose contents are mutated in this module`)
        return
      }
      if (!isPureConstInitializer(ts, ctx.moduleConsts.get(name))) {
        addDep(name, '(same-file const)', false)
      }
      return
    }
    if (ctx.moduleMutables.has(name)) {
      impure.add(`reads module-level let/var '${name}'`)
      return
    }
    addDep(name, '(unresolved global)')
  }

  const walk = (node, parent) => {
    if (ts.isTypeNode(node) || ts.isTypeAliasDeclaration(node) || ts.isInterfaceDeclaration(node)) {
      return
    }
    const k = ts.SyntaxKind
    if (node.kind === k.AwaitExpression) {
      runtime.add('await')
    } else if (node.kind === k.YieldExpression) {
      runtime.add('generator yield')
    } else if (node.kind === k.ThisKeyword) {
      impure.add('uses this')
    } else if (ts.isCallExpression(node) && node.expression.kind === k.ImportKeyword) {
      runtime.add('dynamic import()')
    } else if (ts.isFunctionLike(node) && node.modifiers?.some((m) => m.kind === k.AsyncKeyword)) {
      runtime.add('async function')
    } else if (ts.isBinaryExpression(node) && ASSIGNMENT_OPS.has(k[node.operatorToken.kind])) {
      const root = rootIdentifierOf(ts, node.left)
      if (
        root &&
        !locals.has(root.text) &&
        (ctx.moduleConsts.has(root.text) ||
          ctx.moduleMutables.has(root.text) ||
          ctx.imports.has(root.text))
      ) {
        impure.add(`assigns module-level '${root.text}'`)
      }
    } else if (
      (ts.isPrefixUnaryExpression(node) || ts.isPostfixUnaryExpression(node)) &&
      (node.operator === k.PlusPlusToken || node.operator === k.MinusMinusToken)
    ) {
      const root = rootIdentifierOf(ts, node.operand)
      if (
        root &&
        !locals.has(root.text) &&
        (ctx.moduleMutables.has(root.text) || ctx.moduleConsts.has(root.text))
      ) {
        impure.add(`mutates module-level '${root.text}'`)
      }
    } else if (ts.isDeleteExpression(node)) {
      const root = rootIdentifierOf(ts, node.expression)
      if (root && !locals.has(root.text)) {
        impure.add(`deletes from non-local '${root.text}'`)
      }
    } else if (
      ts.isCallExpression(node) &&
      ts.isPropertyAccessExpression(node.expression) &&
      MUTATING_METHODS.has(node.expression.name.text)
    ) {
      const root = rootIdentifierOf(ts, node.expression.expression)
      if (
        root &&
        !locals.has(root.text) &&
        (ctx.moduleConsts.has(root.text) || ctx.imports.has(root.text))
      ) {
        impure.add(`mutates module-level '${root.text}' via .${node.expression.name.text}()`)
      }
    } else if (ts.isIdentifier(node) && isValueRead(ts, node, parent)) {
      handleIdentifierRead(node, parent)
    }
    ts.forEachChild(node, (child) => walk(child, node))
  }
  if (fn.modifiers?.some((m) => m.kind === ts.SyntaxKind.AsyncKeyword)) {
    runtime.add('async function')
  }
  if (fn.asteriskToken) {
    runtime.add('generator')
  }
  if (fn.body) {
    ts.forEachChild(fn.body, (child) => walk(child, fn.body))
  }

  const callees = [...deps.values()]
  if (runtime.size > 0) {
    return { cls: 'runtime', reasons: [...runtime], callees }
  }
  if (impure.size > 0) {
    return { cls: 'impure', reasons: [...impure], callees }
  }
  if (callees.length > 0) {
    const crossModule = callees.some(
      (c) => c.from !== '(same-file)' && c.from !== '(same-file const)'
    )
    return {
      cls: 'needs-inline',
      scope: crossModule ? 'cross-module' : 'same-file',
      reasons: callees.map((c) => `${c.name} <- ${c.from}${c.inlineable ? ' [inlineable]' : ''}`),
      callees
    }
  }
  return { cls: 'pure-self-contained', reasons: [], callees: [] }
}
