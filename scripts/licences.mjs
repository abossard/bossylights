// Which licences may Bossylights depend on? All must be compatible with Apache-2.0.
// Anything not listed fails, so GPL, LGPL and AGPL fail too.

const ALLOWED = new Set([
  "Apache-2.0", "MIT", "MIT-0", "ISC", "0BSD", "BSD-2-Clause", "BSD-3-Clause",
  "Zlib", "BSL-1.0", "Unicode-3.0", "Unicode-DFS-2016", "CC0-1.0", "Unlicense",
  "BlueOak-1.0.0", "CDLA-Permissive-2.0",
  "CC-BY-4.0", // data only (browser tables in the build tools)
  "MPL-2.0", // file-level copyleft: fine to depend on unchanged
]);
const ALLOWED_EXCEPTIONS = new Set(["LLVM-exception"]);

/** Is an SPDX expression like "(MIT OR Apache-2.0) AND Unicode-3.0" allowed? */
export function allowed(expression) {
  const tokens = expression.replaceAll("/", " OR ").match(/\(|\)|[^\s()]+/g) ?? [];
  let at = 0;
  const peek = () => tokens[at];

  const term = () => {
    if (peek() === "(") {
      at++;
      const inner = or();
      if (tokens[at++] !== ")") throw new Error(`unbalanced: ${expression}`);
      return inner;
    }
    let ok = ALLOWED.has(tokens[at++]);
    if (peek() === "WITH") {
      at++;
      ok = ok && ALLOWED_EXCEPTIONS.has(tokens[at++]);
    }
    return ok;
  };
  const and = () => {
    let ok = term();
    while (peek() === "AND") { at++; ok = term() && ok; }
    return ok;
  };
  const or = () => {
    let ok = and();
    while (peek() === "OR") { at++; ok = and() || ok; }
    return ok;
  };

  if (tokens.length === 0) return false;
  const ok = or();
  return ok && at === tokens.length;
}
