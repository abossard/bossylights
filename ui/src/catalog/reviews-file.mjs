// The owner's reviews are also kept in a file of the repository, so whoever works on the catalog next
// reads them there and not in one browser's store. A plugin of the catalog's dev server:
// GET /reviews.json gives the file, PUT puts one review into it (`{ id, review }`). Runs in Node, so it is JavaScript: ui/ has no Node types.
import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const FILE = fileURLToPath(new URL("../../../docs/design/catalog-reviews.json", import.meta.url));

/** The text to keep for a body that was put, names in order. Undefined when the body is not a map. */
export function fileText(body) {
  let raw;
  try {
    raw = JSON.parse(body);
  } catch {
    return undefined;
  }
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return undefined;
  const inOrder = Object.fromEntries(Object.entries(raw).sort(([a], [b]) => a.localeCompare(b)));
  return `${JSON.stringify(inOrder, null, 2)}\n`;
}

const isReview = (value) =>
  typeof value === "object" && value !== null && Number.isInteger(value.rating) && value.rating >= 0 && value.rating <= 5 && typeof value.comment === "string";

/**
 * The file after one review is put into it: `{ id, review }`, a review of `null` takes it back. A calculation.
 * One control at a time, so a second tab never writes over what the first one rated. Undefined: the body is not one review.
 */
export function withReview(file, body) {
  let one;
  try {
    one = JSON.parse(body);
  } catch {
    return undefined;
  }
  if (typeof one !== "object" || one === null || typeof one.id !== "string" || one.id === "" || !(one.review === null || isReview(one.review))) return undefined;
  let all;
  try {
    all = JSON.parse(file);
  } catch {
    all = {};
  }
  const { [one.id]: _old, ...rest } = typeof all === "object" && all !== null && !Array.isArray(all) ? all : {};
  return fileText(JSON.stringify(one.review === null ? rest : { ...rest, [one.id]: one.review }));
}

const end = (res, status) => {
  res.statusCode = status;
  res.end();
};

/** One write after the other: each reads the file the last one wrote. */
let writing = Promise.resolve();

/** With `CATALOG_REVIEWS=off` it keeps nothing and reads nothing: the headless run rates controls and must not touch the owner's file. */
export function reviewsFile() {
  return {
    name: "catalog-reviews-file",
    configureServer(server) {
      const keeps = process.env.CATALOG_REVIEWS !== "off";
      server.middlewares.use("/reviews.json", (req, res) => {
        if (req.method === "PUT") {
          let body = "";
          req.setEncoding("utf8");
          req.on("data", (chunk) => (body += chunk));
          req.on("end", () => {
            if (withReview("{}", body) === undefined) return end(res, 400);
            if (!keeps) return end(res, 204);
            writing = writing.then(() =>
              readFile(FILE, "utf8")
                .catch(() => "{}")
                .then((file) => writeFile(FILE, withReview(file, body) ?? file))
                .then(() => end(res, 204), () => end(res, 500)),
            );
          });
          return;
        }
        (keeps ? readFile(FILE, "utf8") : Promise.resolve("{}"))
          .catch(() => "{}")
          .then((text) => {
            res.setHeader("Content-Type", "application/json");
            res.setHeader("Cache-Control", "no-store");
            res.end(text);
          });
      });
    },
  };
}
