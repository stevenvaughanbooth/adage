/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Where the teaser films are served from, ending in "/" (e.g. a Vercel Blob store). Default: /media/. */
  readonly VITE_MEDIA_BASE?: string;
  /** The night and day cuts by full URL; each overrides VITE_MEDIA_BASE for its cut. */
  readonly VITE_FILM_DARK_URL?: string;
  readonly VITE_FILM_DAY_URL?: string;
  /** "off" builds the site without the opening film. */
  readonly VITE_FILM?: string;
  /** The public repository, for the README and lexicon links. Unset: the links are hidden. */
  readonly VITE_REPO_URL?: string;
}
declare module "./hero/scene.js" {
  export function build(opts: { canvas: HTMLCanvasElement; labels: HTMLElement; onLost: () => void; light: boolean; held: boolean; reduced: boolean }): unknown;
}
