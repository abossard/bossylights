// The inventory of every interactive element of the mockups, read from their code (issue #102).
// Calculations only: source text in, a list of elements out. `write-inventory.mjs` reads and writes the files.
import { parseSync } from "vite";

/** Intrinsic tags that are interactive by themselves. */
const INTERACTIVE_TAGS = new Set(["button", "input", "select", "textarea", "a", "summary"]);
const INTERACTIVE_ROLES = new Set(["button", "slider", "switch", "radio", "tab", "checkbox", "option", "link", "textbox", "menuitem"]);

/** The gesture a handler stands for, in the words of the behaviour table. */
const GESTURES = [
  [/^on(Click)$/, "press"],
  [/^on(DblClick)$/, "double press"],
  [/^on(PointerDown|PointerUp|PointerCancel|MouseDown|MouseUp|Down|Up)$/, "press and hold"],
  [/^on(PointerMove|MouseMove|Move)$/, "drag"],
  [/^on(DragStart|DragOver|DragEnter|DragLeave|DragEnd|Drop)$/, "drag and drop"],
  [/^on(KeyDown|KeyUp|KeyPress)$/, "keyboard"],
  [/^on(Input|Change|Blur|Focus)$/, "type or pick"],
  [/^on(Wheel)$/, "wheel"],
  [/^on(ContextMenu)$/, "long press"],
];
const LISTENED = new Set(["keydown", "keyup", "pointerdown", "pointerup", "pointermove", "click", "dblclick", "wheel", "blur"]);

/** Every node under `node`, depth first, each with its ancestors (nearest last). */
function* walk(node, ancestors = []) {
  if (!node || typeof node.type !== "string") return;
  yield [node, ancestors];
  const inner = [...ancestors, node];
  for (const key of Object.keys(node)) {
    const value = node[key];
    if (Array.isArray(value)) for (const child of value) yield* walk(child, inner);
    else if (value && typeof value === "object") yield* walk(value, inner);
  }
}

const tagName = (name) =>
  name.type === "JSXMemberExpression" ? `${tagName(name.object)}.${name.property.name}` : name.name;

const isComponent = (tag) => /^[A-Z]/.test(tag);
const isHandler = (name) => /^on[A-Z]/.test(name);

/** The attributes of an opening element: name to value node (true for a bare attribute). */
function attributesOf(opening) {
  const found = new Map();
  for (const attr of opening.attributes) {
    if (attr.type !== "JSXAttribute") continue;
    const name = attr.name.type === "JSXNamespacedName" ? `${attr.name.namespace.name}:${attr.name.name.name}` : attr.name.name;
    found.set(name, attr.value === null ? true : attr.value.type === "JSXExpressionContainer" ? attr.value.expression : attr.value);
  }
  return found;
}

const literal = (node) => (node && node !== true && node.type === "Literal" && typeof node.value === "string" ? node.value : undefined);

/** A label as people read it: literals as they are, anything computed as "…". */
function wording(node, source) {
  if (!node || node === true) return "";
  if (node.type === "Literal") return String(node.value ?? "");
  if (node.type === "JSXText") return node.value;
  if (node.type === "JSXExpressionContainer") return wording(node.expression, source);
  if (node.type === "JSXEmptyExpression") return "";
  if (node.type === "TemplateLiteral") {
    return node.quasis.map((q, i) => q.value.cooked + (i < node.expressions.length ? "…" : "")).join("");
  }
  if (node.type === "ConditionalExpression") return `${wording(node.consequent, source)} | ${wording(node.alternate, source)}`;
  if (node.type === "LogicalExpression") return wording(node.right, source);
  if (node.type === "JSXElement" || node.type === "JSXFragment") {
    const tag = node.type === "JSXElement" ? tagName(node.openingElement.name) : "";
    if (tag === "svg") return "";
    return node.children.map((c) => wording(c, source)).join(" ");
  }
  return "…";
}

const tidy = (text) =>
  text
    .replace(/\s+/g, " ")
    .replace(/(…\s*)+/g, "… ")
    .replace(/(\|\s*)+$/g, "")
    .replace(/\s+([,.:])/g, "$1")
    .trim();

const slug = (text) =>
  text
    .split("|")[0]
    .toLowerCase()
    .replace(/…/g, " ")
    .replace(/[^a-z0-9]+/g, " ")
    .trim()
    .split(" ")
    .filter(Boolean)
    .slice(0, 5)
    .join("-");

/** The dotted name of a callee, or undefined: `store.tap`, `props.onChange`, `press`. */
function calleeName(node) {
  if (node.type === "Identifier") return node.name;
  if (node.type === "ThisExpression") return "this";
  if (node.type === "ChainExpression") return calleeName(node.expression);
  if (node.type === "TSNonNullExpression") return calleeName(node.expression);
  if (node.type === "MemberExpression" && !node.computed) {
    const object = calleeName(node.object);
    return object ? `${object}.${node.property.name}` : undefined;
  }
  return undefined;
}

/** The functions a file defines by name, so `onClick={press}` can be followed to what `press` does. */
function localFunctions(ast) {
  const found = new Map();
  for (const [node] of walk(ast)) {
    if (node.type === "FunctionDeclaration" && node.id) found.set(node.id.name, node.body);
    if (node.type === "VariableDeclarator" && node.id.type === "Identifier" && node.init) {
      if (node.init.type === "ArrowFunctionExpression" || node.init.type === "FunctionExpression") {
        found.set(node.id.name, node.init.body);
      }
    }
  }
  return found;
}

/**
 * What the handlers of an element call, followed through the file's own functions:
 * `sends` (the fake store, the mockups' stand-in for the engine), `passes` (a callback from the
 * component's props: the parent decides) and `ui` (everything else: screen-only state, navigation).
 */
function callsOf(expressions, locals) {
  const sends = new Set();
  const passes = new Set();
  const ui = new Set();
  const seen = new Set();
  const follow = (name) => {
    if (!locals.has(name) || seen.has(name)) return locals.has(name);
    seen.add(name);
    visit(locals.get(name));
    return true;
  };
  const note = (name) => {
    if (!name) return;
    const parts = name.split(".");
    const [head] = parts;
    const last = parts[parts.length - 1];
    if (head === "store" && parts.length > 1) sends.add(parts[1]);
    else if (["props", "own", "rest"].includes(head) && parts.length > 1 && !follow(last)) passes.add(parts[1]);
    else if ((parts.length === 1 || !BUILT_IN.has(last)) && follow(last)) return;
    else if (parts.length > 1 ? /^(review|learn)$/.test(head) : UI_VERB.test(name)) ui.add(name);
  };
  const visit = (root) => {
    if (!root || root === true) return;
    if (root.type === "Identifier" || root.type === "MemberExpression") note(calleeName(root));
    for (const [node] of walk(root)) {
      if (node.type === "CallExpression") note(calleeName(node.callee));
    }
  };
  for (const expression of expressions) visit(expression);
  const sorted = (set) => [...set].sort();
  return { sends: sorted(sends), passes: sorted(passes), ui: sorted(ui) };
}

/** Method names of sets, maps, arrays and elements: `seen.add(x)` is not a call to a function named `add`. */
const BUILT_IN = new Set("add delete set get has clear push pop filter map find some every includes forEach slice join sort select focus remove".split(" "));

/** A call that changes what the screen shows and nothing else: a signal setter, a jump. */
const UI_VERB = /^(set[A-Z]|go$|open|close|select|pick|drill|fold)/;

/** Where a file sits in the mockups: a screen id, or shell, kit, review, app. */
export function areaOf(file) {
  const screen = /^screens\/([^/]+)\//.exec(file);
  if (screen) return screen[1];
  const folder = /^([^/]+)\//.exec(file);
  return folder ? folder[1] : "app";
}

/** The nearest function with a component's name around a node. */
function componentOf(ancestors) {
  for (let i = ancestors.length - 1; i >= 0; i -= 1) {
    const node = ancestors[i];
    if (node.type === "FunctionDeclaration" && node.id && isComponent(node.id.name)) return node.id.name;
    if (node.type === "VariableDeclarator" && node.id.type === "Identifier" && isComponent(node.id.name)) return node.id.name;
  }
  return "";
}

/** The id of the nearest `<Region id="...">` around a node. */
function regionOf(ancestors) {
  for (let i = ancestors.length - 1; i >= 0; i -= 1) {
    const node = ancestors[i];
    if (node.type !== "JSXElement" || tagName(node.openingElement.name) !== "Region") continue;
    return literal(attributesOf(node.openingElement).get("id")) ?? "";
  }
  return "";
}

/** Components that hand all their props to an interactive tag: every use of them is a control. */
export function spreadingComponents(sources) {
  const found = new Set();
  for (const [file, source] of Object.entries(sources)) {
    if (!/\.tsx$/.test(file)) continue;
    for (const [node, ancestors] of walk(parseSync(file, source).program)) {
      if (node.type !== "JSXOpeningElement" || !INTERACTIVE_TAGS.has(tagName(node.name))) continue;
      if (node.attributes.some((a) => a.type === "JSXSpreadAttribute")) found.add(componentOf(ancestors));
    }
  }
  found.delete("");
  return found;
}

/**
 * The interactive elements of one file, in source order. `spreading`: see `spreadingComponents`.
 *
 * | field     | is                                                                  |
 * |-----------|---------------------------------------------------------------------|
 * | tag       | `button`, `Pad`, `input[range]`, or `window` for a key listener     |
 * | label     | its accessible name or text, computed parts as "…"                  |
 * | gestures  | from its handlers: press, press and hold, drag, keyboard, …         |
 * | sends     | fake-store commands its handlers reach                              |
 * | passes    | callbacks of its own component it calls: the user of it decides     |
 * | ui        | screen-only state it changes                                        |
 */
export function elementsOf(file, source, spreading = new Set(), shared = new Map()) {
  const ast = parseSync(file, source).program;
  const locals = new Map([...shared, ...localFunctions(ast)]);
  const lineAt = (offset) => source.slice(0, offset).split("\n").length;
  const elements = [];

  for (const [node, ancestors] of walk(ast)) {
    if (node.type === "CallExpression") {
      const name = calleeName(node.callee) ?? "";
      if (!/^(window|document)\.addEventListener$/.test(name)) continue;
      const event = literal(node.arguments[0]) ?? "…";
      if (event !== "…" && !LISTENED.has(event)) continue;
      const calls = callsOf(node.arguments.slice(1), locals);
      elements.push({
        tag: name.split(".")[0],
        label: event,
        role: "",
        gestures: [/^key/.test(event) ? "keyboard" : "press"],
        ...calls,
        component: componentOf(ancestors) || enclosingFunction(ancestors),
        region: "",
        line: lineAt(node.start),
      });
      continue;
    }
    if (node.type !== "JSXOpeningElement") continue;
    const tag = tagName(node.name);
    const attrs = attributesOf(node);
    const handlers = [...attrs.keys()].filter(isHandler);
    const role = literal(attrs.get("role")) ?? "";
    const spreads = node.attributes.some((a) => a.type === "JSXSpreadAttribute");
    const interactive = isComponent(tag)
      ? handlers.length > 0 || spreading.has(tag)
      : INTERACTIVE_TAGS.has(tag) ||
        handlers.some((h) => h !== "onContextMenu") ||
        INTERACTIVE_ROLES.has(role) ||
        attrs.has("draggable") ||
        attrs.has("contentEditable") ||
        attrs.has("tabIndex");
    if (!interactive) continue;

    const parent = ancestors[ancestors.length - 1];
    const named = ["aria-label", "label", "title", "placeholder"].map((name) => attrs.get(name)).find(Boolean);
    const text = named ? wording(named, source) : parent?.type === "JSXElement" ? parent.children.map((c) => wording(c, source)).join(" ") : "";
    const type = literal(attrs.get("type"));
    const gestures = new Set(handlers.flatMap((h) => GESTURES.filter(([re]) => re.test(h)).map(([, g]) => g)));
    if (!isComponent(tag) && (tag === "button" || tag === "a" || tag === "summary")) gestures.add("press");
    if (tag === "input" || tag === "select" || tag === "textarea") gestures.add("type or pick");
    if (attrs.has("draggable")) gestures.add("drag and drop");

    elements.push({
      tag: tag === "input" && type ? `input[${type}]` : tag,
      label: tidy(text),
      role,
      gestures: [...gestures].sort(),
      ...callsOf(handlers.map((h) => attrs.get(h)), locals),
      component: componentOf(ancestors),
      region: regionOf(ancestors),
      line: lineAt(node.start),
      ...(spreads ? { spreads: true } : {}),
    });
  }

  elements.sort((a, b) => a.line - b.line);
  return elements.map((e) => ({ file, ...e }));
}

/** The name of the nearest named function, for listeners installed outside a component. */
function enclosingFunction(ancestors) {
  for (let i = ancestors.length - 1; i >= 0; i -= 1) {
    const node = ancestors[i];
    if (node.type === "FunctionDeclaration" && node.id) return node.id.name;
  }
  return "";
}

/**
 * Every interactive element of the mockups, each with a stable key.
 * `sources`: { path relative to mockups/src: text }.
 *
 * The key is `area/Component/what-it-says`, numbered when two elements of one component say the same.
 * It does not move when lines move.
 */
export function inventory(sources) {
  const files = Object.keys(sources).filter((f) => /\.tsx?$/.test(f)).sort();
  const spreading = spreadingComponents(sources);
  // functions of the other files of the same screen, so a call into one of them is followed too
  const functions = new Map(files.map((f) => [f, [...localFunctions(parseSync(f, sources[f]).program)]]));
  const sharedWith = (file) => new Map(files.filter((f) => areaOf(f) === areaOf(file)).flatMap((f) => functions.get(f)));
  const taken = new Map();
  return files.flatMap((file) =>
    elementsOf(file, sources[file], spreading, sharedWith(file)).map((element) => {
      const what = slug(element.label) || element.tag.toLowerCase().replace(/[^a-z]+/g, "-").replace(/-$/, "");
      const base = [areaOf(file), element.component || file.replace(/^.*\//, "").replace(/\.tsx?$/, ""), what].join("/");
      const count = (taken.get(base) ?? 0) + 1;
      taken.set(base, count);
      return { key: count === 1 ? base : `${base}#${count}`, ...element };
    }),
  );
}
