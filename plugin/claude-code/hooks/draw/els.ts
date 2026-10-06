// The elements a drawing takes: the ones the terminal and the desktop both have, so one tree
// validates on both (ux-design §4.12; T-X-07). The `Mod`'s `ui.resolve` hands them over.
import type { Elements, RenderElement } from 'claude-code'

export type Els = Pick<Elements['terminal'], 'Box' | 'Text' | 'Button' | 'Input' | 'Select' | 'Link' | 'Markdown' | 'Code'>

export type Tree = RenderElement
