import v5Page from "./v5.html";
import v6Page from "./v6.html";

const server = Bun.serve({
  port: Number(process.env.PORT ?? 3000),
  routes: {
    "/": v6Page,
    "/v5": v5Page,
    "/v6": v6Page,
  },
  development: {
    hmr: true,
    console: true,
  },
});

console.log(`POC server ready on ${server.url}`);
console.log(`  v5: ${server.url}v5`);
console.log(`  v6: ${server.url}v6  (also /)`);
