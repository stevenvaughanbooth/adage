// A stand-in chapter: its steps over the sample traces, before the chapter has its own scene.
// Every chapter starts here, so the tab is complete while scenes are built one by one.

import type { DotLook, Stage } from "../engine";
import { PROTOS, TRACES, laneX, laneZ } from "../sample";
import type { Chapter, Chip, Status } from "../types";

interface Beat {
  title: string;
  body: string;
  chips?: string[];
}

export function storyboard(c: { id: string; title: string; status: string; kind: Status; summary: string; source: string; beats: Beat[] }): Chapter {
  return {
    ...c,
    technical: c.title,
    usesRepo: false,
    build(stage: Stage) {
      const ids: string[] = [];
      TRACES.forEach((tr, i) =>
        tr.forEach((_, j) => {
          const id = `n${i}.${j}`;
          stage.dot(id);
          ids.push(id);
        }),
      );
      const pairs: [string, string][] = [];
      TRACES.forEach((tr, i) => tr.forEach((_, j) => j && pairs.push([`n${i}.${j - 1}`, `n${i}.${j}`])));
      stage.link("lane", pairs, "ink3");
      return {
        source: `${TRACES.length} sample traces`,
        steps: c.beats.map((b, s) => {
          const dots: Record<string, DotLook> = {};
          TRACES.forEach((tr, i) =>
            tr.forEach(([code], j) => {
              dots[`n${i}.${j}`] = { pos: [laneX(j), 0, laneZ(i)], color: { hue: PROTOS[code].hue }, alpha: 0.7, glow: 0.12 };
            }),
          );
          const chips: Chip[] = (b.chips ?? []).map((text) => ({ text }));
          return {
            title: b.title,
            plain: b.body,
            body: b.body,
            chips,
            state: { dots, links: { lane: 0.3 }, cam: { target: [-0.6, -1, 0.6], yaw: -0.2 + s * 0.08, pitch: 0.75, dist: 25 } },
          };
        }),
      };
    },
  };
}
