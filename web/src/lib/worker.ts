// Runs long solver calls off the main thread. Cancel = terminate this worker.
import init, * as w from '../wasm/crew_wasm.js';

const ready = init();

type Call = { id: number; fn: string; args: unknown[]; trace?: boolean };

self.onmessage = async (e: MessageEvent<Call>) => {
  await ready;
  const { id, fn, args, trace } = e.data;
  const progress = (...p: number[]) => postMessage({ id, progress: p });
  // Trace events, stamped with the time here (the solver has no clock).
  w.set_trace(trace ? (ev: string) => postMessage({ id, trace: { ...JSON.parse(ev), t: performance.now() + performance.timeOrigin } }) : null);
  const decided = (draft: string) => postMessage({ id, decided: JSON.parse(draft) });
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
        out = w.drafts(args[0] as string, args[1] as boolean, args[2] as number, args[3] as number, args[4] as number, progress, decided);
        break;
      case 'pair_conflicts':
        out = w.pair_conflicts(args[0] as string, args[1] as boolean, args[2] as number, args[3] as number, progress);
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
      case 'deal_pairing':
        out = w.deal_pairing(args[0] as string, args[1] as number, args[2] as number, args[3] as number, args[4] as number, decided, progress);
        break;
      case 'deal_robustness':
        out = w.deal_robustness(args[0] as string, args[1] as number, args[2] as number, args[3] as number, args[4] as number, progress, decided);
        break;
      default:
        out = JSON.stringify({ error: { msg: `unknown function ${fn}` } });
    }
  } catch (err) {
    out = JSON.stringify({ error: { msg: String(err) } });
  }
  postMessage({ id, result: JSON.parse(out) });
};
