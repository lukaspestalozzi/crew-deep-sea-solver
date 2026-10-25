// Runs long solver calls off the main thread. Cancel = terminate this worker.
import init, * as w from '../wasm/crew_wasm.js';

const ready = init();

type Call = { id: number; fn: string; args: unknown[] };

self.onmessage = async (e: MessageEvent<Call>) => {
  await ready;
  const { id, fn, args } = e.data;
  const progress = (...p: number[]) => postMessage({ id, progress: p });
  let out: string;
  try {
    switch (fn) {
      case 'solve':
        out = w.solve(args[0] as string, progress);
        break;
      case 'solve_drafts':
        out = w.solve_drafts(args[0] as string, progress);
        break;
      case 'drafts':
        out = w.drafts(args[0] as string, args[1] as boolean, args[2] as number, args[3] as number, args[4] as number, progress);
        break;
      case 'conflict':
        out = w.conflict(args[0] as string);
        break;
      case 'no_return':
        out = w.no_return(args[0] as string);
        break;
      case 'tightness':
        out = w.tightness(args[0] as string, args[1] as string);
        break;
      case 'deal_robustness':
        out = w.deal_robustness(args[0] as string, args[1] as number, args[2] as number, args[3] as number, args[4] as number, progress);
        break;
      default:
        out = JSON.stringify({ error: { msg: `unknown function ${fn}` } });
    }
  } catch (err) {
    out = JSON.stringify({ error: { msg: String(err) } });
  }
  postMessage({ id, result: JSON.parse(out) });
};
