/**
 * 本地开发服务器
 * - 托管 HTML 页面（解决 file:// 不能发 fetch 的问题）
 * - /api/proxy 代理转发请求到任意 API 地址（彻底绕过 CORS）
 *
 * 启动：node server.js
 * 访问：http://localhost:3000
 */

const http = require('http');
const fs   = require('fs');
const path = require('path');

const PORT = 3000;
const HTML_FILE = path.join(__dirname, 'vlm-detector.html');

const MIME = {
    '.html': 'text/html; charset=utf-8',
    '.js':   'application/javascript',
    '.css':  'text/css',
    '.json': 'application/json',
    '.png':  'image/png',
    '.jpg':  'image/jpeg',
    '.svg':  'image/svg+xml',
};

const server = http.createServer(async (req, res) => {
    // ---------- CORS 头（让前端可以任意访问） ----------
    res.setHeader('Access-Control-Allow-Origin',  '*');
    res.setHeader('Access-Control-Allow-Methods', 'GET, POST, OPTIONS');
    res.setHeader('Access-Control-Allow-Headers', 'Content-Type, Authorization');

    if (req.method === 'OPTIONS') {
        res.writeHead(204);
        return res.end();
    }

    // ---------- 代理端点：POST /api/proxy ----------
    // 客户端发送: { url: "https://...", headers: {...}, body: {...} }
    if (req.method === 'POST' && req.url === '/api/proxy') {
        let raw = '';
        req.on('data', chunk => raw += chunk);
        req.on('end', async () => {
            try {
                const { url, headers, body } = JSON.parse(raw);

                if (!url) {
                    res.writeHead(400, { 'Content-Type': 'application/json' });
                    return res.end(JSON.stringify({ error: '缺少 url 参数' }));
                }

                console.log(`[proxy] → ${url}`);

                const fetchHeaders = { ...(headers || {}) };
                // 确保 host 头指向目标而非 localhost
                delete fetchHeaders['host'];
                delete fetchHeaders['Host'];

                const resp = await fetch(url, {
                    method: 'POST',
                    headers: fetchHeaders,
                    body: body ? JSON.stringify(body) : undefined,
                });

                const text = await resp.text();
                res.writeHead(resp.status, {
                    'Content-Type': resp.headers.get('content-type') || 'application/json',
                });
                res.end(text);

            } catch (err) {
                console.error('[proxy error]', err.message);
                res.writeHead(502, { 'Content-Type': 'application/json' });
                res.end(JSON.stringify({ error: err.message }));
            }
        });
        return;
    }

    // ---------- 静态文件：托管 HTML ----------
    if (req.method === 'GET' && (req.url === '/' || req.url === '/index.html')) {
        try {
            const content = fs.readFileSync(HTML_FILE, 'utf-8');
            const ext = path.extname(HTML_FILE);
            res.writeHead(200, { 'Content-Type': MIME[ext] || 'text/plain' });
            res.end(content);
        } catch (err) {
            res.writeHead(404);
            res.end('Not found: ' + HTML_FILE);
        }
        return;
    }

    res.writeHead(404);
    res.end('Not found');
});

server.listen(PORT, () => {
    console.log('');
    console.log('  🔍 VLM 人体检测服务已启动');
    console.log(`  👉 打开浏览器访问: http://localhost:${PORT}`);
    console.log('');
    console.log('  按 Ctrl+C 停止');
    console.log('');
});
