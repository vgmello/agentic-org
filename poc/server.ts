import v1Page from "./v1.html";
import v2Page from "./v2.html";
import v3Page from "./v3.html";
import v4Page from "./v4.html";
import v5BetaPage from "./v5-beta.html";
import v5Beta1Page from "./v5-beta1.html";
import v5Page from "./v5.html";
import v6Page from "./v6.html";

const server = Bun.serve({
  port: Number(process.env.PORT ?? 3000),
  routes: {
    "/": v4Page,
    "/v1": v1Page,
    "/v2": v2Page,
    "/v3": v3Page,
    "/v4": v4Page,
    "/v5-beta": v5BetaPage,
    "/v5-beta1": v5Beta1Page,
    "/v5": v5Page,
    "/v6": v6Page,
    "/before": v1Page,
  },
  development: {
    hmr: true,
    console: true,
  },
});

console.log(`POC server ready on ${server.url}`);
console.log(`  v1 (original):   ${server.url}v1`);
console.log(`  v2 (navy dark):  ${server.url}v2`);
console.log(`  v3 (orb tone):   ${server.url}v3`);
console.log(`  v4 (elevated):   ${server.url}v4  (also /)`);
console.log(`  v5-beta (base):  ${server.url}v5-beta`);
console.log(`  v5-beta1 (it 1): ${server.url}v5-beta1`);
console.log(`  v5 (frosted):    ${server.url}v5`);
console.log(`  v6 (ai native):  ${server.url}v6`);
