import indexPage from "./index.html";
import beforePage from "./before.html";

const server = Bun.serve({
  port: Number(process.env.PORT ?? 3000),
  routes: {
    "/": indexPage,
    "/before": beforePage,
  },
  development: {
    hmr: true,
    console: true,
  },
});

console.log(`POC server ready on ${server.url}`);
console.log(`  Current: ${server.url}`);
console.log(`  Before:  ${server.url}before`);
