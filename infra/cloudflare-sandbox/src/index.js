import { DurableObject } from "cloudflare:workers";
import { route, Session } from "./protocol.js";

export class StruoSandbox extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    this.session = new Session(ctx);
    ctx.blockConcurrencyWhile(async () => {
      if (ctx.container.running) await ctx.container.setInactivityTimeout(3_600_000);
    });
  }

  fetch(request) { return this.session.fetch(request); }
  alarm() { return this.session.destroy(); }
}

export default { fetch: route };
