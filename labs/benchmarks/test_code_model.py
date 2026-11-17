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
    "model": "cohere/north-mini-code:free",
    "messages": [
        {"role": "user", "content": "Respond strictly with JSON: {\"status\": \"ready\", \"specialty\": \"code\"}"}
    ],
    "temperature": 0.1,
    "max_tokens": 100
}

req = urllib.request.Request(url, data=json.dumps(payload).encode("utf-8"), headers=headers, method="POST")
try:
    with urllib.request.urlopen(req, timeout=30) as resp:
        data = json.loads(resp.read().decode("utf-8"))
        print(json.dumps(data, indent=2))
except Exception as e:
    print("Error:", e)
