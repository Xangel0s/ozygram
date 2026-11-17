import os
import sys
import json
import urllib.request
import urllib.error

API_KEY = os.environ.get("OPENROUTER_API_KEY") or (sys.argv[1] if len(sys.argv) > 1 else None)
if not API_KEY:
    print("Error: Missing OPENROUTER_API_KEY", file=sys.stderr)
    sys.exit(1)

url = "https://openrouter.ai/api/v1/chat/completions"
headers = {
    "Authorization": f"Bearer {API_KEY}",
    "Content-Type": "application/json",
    "HTTP-Referer": "https://github.com/Xangel0s/ozygram",
    "X-Title": "Ozygram Benchmark"
}

payload = {
    "model": "deepseek/deepseek-chat:free",
    "messages": [
        {"role": "user", "content": "Respond with exact JSON: {\"status\": \"ok\", \"model\": \"deepseek-chat\"}"}
    ],
    "temperature": 0.1,
    "max_tokens": 100
}

req = urllib.request.Request(url, data=json.dumps(payload).encode("utf-8"), headers=headers, method="POST")

try:
    with urllib.request.urlopen(req, timeout=30) as resp:
        data = json.loads(resp.read().decode("utf-8"))
        print("HTTP Status:", resp.status)
        print("Response Content:")
        print(json.dumps(data, indent=2))
except urllib.error.HTTPError as e:
    err_body = e.read().decode("utf-8")
    print(f"HTTP Error {e.code}: {err_body}", file=sys.stderr)
    sys.exit(1)
except Exception as e:
    print(f"Unexpected error: {e}", file=sys.stderr)
    sys.exit(1)
