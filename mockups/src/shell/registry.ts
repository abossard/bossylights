// Screens are found by convention: every src/screens/<id>/index.tsx is one
// screen. Adding or replacing a screen needs no edit outside its folder.
import type { Component } from "solid-js";

export interface Issue {
  number: number;
  title: string;
}

export interface ScreenModule {
  /** Matches the folder name. Used in the URL (#/<id>) and in review exports. */
  id: string;
  /** Plain-words tab title. */
  title: string;
  /** Position among the workspace tabs. Also its number key (1 to 9). */
  order: number;
  /** The GitHub issues this screen stands for. */
  issues: Issue[];
  Screen: Component;
  /** Set false on screens that show the full timetable themselves. Default true. */
  nowStrip?: boolean;
}

const modules = import.meta.glob<ScreenModule>("../screens/*/index.tsx", { eager: true });

export const screens: ScreenModule[] = Object.entries(modules)
  .map(([path, m]) => {
    const folder = path.split("/").at(-2);
    if (m.id !== folder) console.warn(`screen in ${folder}/ exports id "${m.id}": use the folder name`);
    return m;
  })
  .sort((a, b) => a.order - b.order);

export const screenById = (id: string) => screens.find((s) => s.id === id);

export const ISSUE_URL = "https://github.com/abossard/bossylights/issues/";
