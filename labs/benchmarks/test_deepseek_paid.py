import os
import json
import urllib.request
import urllib.error

API_KEY = os.environ.get("OPENROUTER_API_KEY")
url = "https://openrouter.ai/api/v1/chat/completions"
headers = {
    "Authorization": f"Bearer {API_KEY}",
    "Content-Type": "application/json",
}

payload = {
    "model": "deepseek/deepseek-chat",
    "messages": [{"role": "user", "content": "hi"}],
    "max_tokens": 10
}

req = urllib.request.Request(url, data=json.dumps(payload).encode("utf-8"), headers=headers, method="POST")
try:
    with urllib.request.urlopen(req, timeout=30) as resp:
        print("Success:", resp.read().decode("utf-8"))
except urllib.error.HTTPError as e:
    print(f"Error {e.code}: {e.read().decode('utf-8')}")
