import type { Plugin } from "vite";

export function fileText(body: string): string | undefined;
export function withReview(file: string, body: string): string | undefined;
export function reviewsFile(): Plugin;
