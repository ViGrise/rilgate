import { createServer } from 'node:http';

const port = Number(process.argv[2] ?? 3300);
const body = Buffer.alloc(1024, 'x');
const server = createServer((request, response) => {
  request.resume();
  request.on('end', () => {
    response.writeHead(200, {
      'content-type': 'text/plain',
      'content-length': String(body.length),
    });
    response.end(body);
  });
});
server.keepAliveTimeout = 60_000;
server.listen(port, '127.0.0.1');
