import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import ts from "typescript";

export function frontendFiles(dir: string): ts.SourceFile[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return frontendFiles(full);
    if (!/\.(ts|tsx)$/.test(entry.name) || /\.test\.tsx?$/.test(entry.name))
      return [];
    return [
      ts.createSourceFile(
        full,
        readFileSync(full, "utf8"),
        ts.ScriptTarget.Latest,
        true,
        entry.name.endsWith("x") ? ts.ScriptKind.TSX : ts.ScriptKind.TS,
      ),
    ];
  });
}

export function walk(node: ts.Node, visit: (node: ts.Node) => void) {
  visit(node);
  node.forEachChild((child) => walk(child, visit));
}
