// Stand-in for the generated bindings (src/wasm, gitignored; `npm run wasm`),
// so `npm run check` also works without them. tsconfig `rootDirs` makes the
// generated files win when they exist.
type Cb = ((s: string) => void) | ((...n: number[]) => void) | null;
export default function init(input?: unknown): Promise<unknown>;
export function set_trace(f: Cb): void;
export function catalogue_json(): string;
export function task_info(id: string): string;
export function parse(text: string): string;
export function write(dtos_json: string): string;
export function check(text: string): string;
export function solve(text: string, progress: Cb): string;
export function solve_drafts(text: string, progress: Cb): string;
export function drafts(text: string, all_drafts: boolean, limit: number, part: number, parts: number, progress: Cb, decided: Cb): string;
export function conflict(text: string): string;
export function no_return(text: string): string;
export function tightness(text: string, line_json: string): string;
export function pair_conflicts(text: string, all_drafts: boolean, part: number, parts: number, progress: Cb): string;
export function deal_robustness(text: string, deals: number, seed: number, part: number, parts: number, progress: Cb, on_deal: Cb): string;
export function deal_pairing(text: string, deals: number, seed: number, part: number, parts: number, on_deal: Cb, progress: Cb): string;
export function position(text: string): string;
export function solve_moves(text: string, only: string, budget: number, part: number, parts: number, on_move: Cb): string;
export function play_out(text: string, order: number, budget: number, on_move: Cb): string;
export function position_stats(text: string, budget: number, part: number, parts: number, on_position: Cb): string;
export function random_situation(players: number, target: number, seed: number): string;
