"""A minimal stand-in for llama.cpp's server, for PROVING Atlas's local-LLM
path end to end here. It speaks the exact two endpoints Atlas uses:

  GET  /health      -> 200 (what models::health_url checks)
  POST /completion  -> {"content": "<generated text>"}   (llama.cpp's shape;
                       Atlas reads the text at response_path "content")

Backed by a real quantized model via llama-cpp-python. This is only the test
harness — on a real machine you run the actual llama.cpp `llama-server`, which
serves these same endpoints; Atlas cannot tell the difference, which is the
point of proving against this.
"""
import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer
from llama_cpp import Llama

MODEL = sys.argv[1]
PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 8080

llm = Llama(model_path=MODEL, n_ctx=2048, verbose=False)


class H(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_GET(self):
        if self.path == "/health":
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b'{"status":"ok"}')
        else:
            self.send_response(404)
            self.end_headers()

    def do_POST(self):
        n = int(self.headers.get("content-length", 0))
        req = json.loads(self.rfile.read(n) or b"{}")
        prompt = req.get("prompt", "")
        stops = req.get("stop", []) or []
        n_predict = int(req.get("n_predict", 256))
        out = llm(prompt, max_tokens=n_predict, stop=stops, temperature=0.2)
        text = out["choices"][0]["text"]
        body = json.dumps({"content": text}).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.end_headers()
        self.wfile.write(body)


if __name__ == "__main__":
    HTTPServer(("127.0.0.1", PORT), H).serve_forever()
