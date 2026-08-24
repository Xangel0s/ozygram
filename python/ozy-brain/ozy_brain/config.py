from __future__ import annotations

import os
from dataclasses import dataclass
from enum import Enum
from typing import Any


class LLMProvider(str, Enum):
    OPENROUTER = "openrouter"
    OLLAMA = "ollama"
    GEMINI = "gemini"
    OPENAI = "openai"
    ANTHROPIC = "anthropic"
    OFFLINE_FALLBACK = "offline_fallback"


OPENROUTER_FREE_MODELS = [
    "openrouter/nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free",
    "openrouter/nvidia/nemotron-3-super-120b-a12b:free",
    "openrouter/cohere/north-mini-code:free",
    "openrouter/z-ai/glm-5.2:free",
    "openrouter/google/gemma-4-31b-it:free",
]


import sys


def _get_env_var(name: str) -> str | None:
    val = os.getenv(name)
    if val:
        return val
    if sys.platform == "win32":
        try:
            import winreg
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Environment") as key:
                reg_val, _ = winreg.QueryValueEx(key, name)
                if reg_val:
                    return str(reg_val)
        except Exception:
            pass
    return None


@dataclass
class BrainConfig:
    provider: LLMProvider
    model: str
    api_key: str | None
    api_base: str | None
    temperature: float = 0.2
    max_tokens: int = 1500

    @classmethod
    def load(cls) -> BrainConfig:
        """Auto-detects the best available LLM provider in cascade order:

        1. OpenRouter (if OPENROUTER_API_KEY is present)
        2. Ollama (if OLLAMA_HOST or OLLAMA_BASE_URL is present)
        3. Gemini (if GEMINI_API_KEY is present)
        4. OpenAI (if OPENAI_API_KEY is present)
        5. Anthropic (if ANTHROPIC_API_KEY is present)
        6. Offline Fallback (Cero costo / Heurístico local)
        """
        custom_model = _get_env_var("OZY_BRAIN_MODEL")
        openrouter_key = _get_env_var("OPENROUTER_API_KEY")

        if openrouter_key:
            return cls(
                provider=LLMProvider.OPENROUTER,
                model=custom_model or "openrouter/nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free",
                api_key=openrouter_key,
                api_base="https://openrouter.ai/api/v1",
            )

        ollama_base = _get_env_var("OLLAMA_BASE_URL") or _get_env_var("OLLAMA_HOST")
        if ollama_base:
            if not ollama_base.startswith("http"):
                ollama_base = f"http://{ollama_base}"
            return cls(
                provider=LLMProvider.OLLAMA,
                model=custom_model or "ollama/qwen2.5-coder",
                api_key=None,
                api_base=ollama_base,
            )

        gemini_key = _get_env_var("GEMINI_API_KEY")
        if gemini_key:
            return cls(
                provider=LLMProvider.GEMINI,
                model=custom_model or "gemini/gemini-2.0-flash",
                api_key=gemini_key,
                api_base=None,
            )

        openai_key = _get_env_var("OPENAI_API_KEY")
        if openai_key:
            return cls(
                provider=LLMProvider.OPENAI,
                model=custom_model or "gpt-4o-mini",
                api_key=openai_key,
                api_base=None,
            )

        anthropic_key = _get_env_var("ANTHROPIC_API_KEY")
        if anthropic_key:
            return cls(
                provider=LLMProvider.ANTHROPIC,
                model=custom_model or "claude-3-5-haiku-20241022",
                api_key=anthropic_key,
                api_base=None,
            )

        return cls(
            provider=LLMProvider.OFFLINE_FALLBACK,
            model="heuristic-offline-v1",
            api_key=None,
            api_base=None,
        )


def call_llm(prompt: str, system_prompt: str = "", config: BrainConfig | None = None) -> str | None:
    """Invokes LLM via LiteLLM with automatic multi-model fallback on OpenRouter free tier."""
    cfg = config or BrainConfig.load()
    if cfg.provider == LLMProvider.OFFLINE_FALLBACK:
        return None

    try:
        import litellm

        # Suppress noisy LiteLLM logs
        litellm.suppress_debug_info = True

        messages = []
        if system_prompt:
            messages.append({"role": "system", "content": system_prompt})
        messages.append({"role": "user", "content": prompt})

        # Candidate models to try in sequence (max 2 to preserve ultra-low latency)
        candidate_models = [cfg.model]
        if cfg.provider == LLMProvider.OPENROUTER and not os.getenv("OZY_BRAIN_MODEL"):
            for m in OPENROUTER_FREE_MODELS[:2]:
                if m not in candidate_models:
                    candidate_models.append(m)

        for model_name in candidate_models[:2]:
            try:
                response = litellm.completion(
                    model=model_name,
                    messages=messages,
                    api_key=cfg.api_key,
                    api_base=cfg.api_base,
                    temperature=cfg.temperature,
                    max_tokens=cfg.max_tokens,
                    timeout=5,
                )
                content = response.choices[0].message.content
                if content and content.strip():
                    return content
            except Exception:
                continue

        return None
    except Exception:
        return None
