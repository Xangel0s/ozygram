import os
import json
import urllib.request

API_KEY = os.environ.get("OPENROUTER_API_KEY")
url = "https://openrouter.ai/api/v1/chat/completions"
headers = {
    "Authorization": f"Bearer {API_KEY}",
    "Content-Type": "application/json",
    "HTTP-Referer": "https://github.com/Xangel0s/ozygram",
    "X-Title": "Ozygram Benchmark"
}

payload = {
    "model": "openrouter/free",
    "messages": [
        {"role": "user", "content": "Respond strictly with JSON: {\"status\": \"ready\", \"model_name\": \"test\"}"}
    ],
    "temperature": 0.1,
    "max_tokens": 100
}

req = urllib.request.Request(url, data=json.dumps(payload).encode("utf-8"), headers=headers, method="POST")
with urllib.request.urlopen(req, timeout=30) as resp:
    data = json.loads(resp.read().decode("utf-8"))
    print(json.dumps(data, indent=2))
