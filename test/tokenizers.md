# LLM Tokenizers for Compression Benchmarking

This document lists major tokenizer implementations that can be run locally to measure token savings from file compression.

## Selection Criteria

- Widely used model family
- Local execution supported
- Industry relevance
- No duplicate tokenizers for the same model family

---

## OpenAI

### tiktoken

- **Provider:** OpenAI
- **Models:** GPT-4o, GPT-4, GPT-3.5, o1, o3, o4 series
- **Description:** Official OpenAI tokenizer implementation. Produces the same token counts used by OpenAI models. Supports `cl100k_base`, `o200k_base`, and other OpenAI encodings.
- **Language:** Rust core with Python and JavaScript bindings
- **Download:**
  - https://github.com/openai/tiktoken
  - https://www.npmjs.com/package/js-tiktoken

---

## Anthropic

### @anthropic-ai/tokenizer

- **Provider:** Anthropic
- **Models:** Legacy Claude models and approximate counting for newer Claude models
- **Description:** Anthropic's publicly released tokenizer package. Useful for local estimation, though modern Claude token counts may differ from API-reported counts.
- **Language:** JavaScript / TypeScript
- **Download:**
  - https://www.npmjs.com/package/@anthropic-ai/tokenizer

---

## Meta

### Llama Tokenizer

- **Provider:** Meta
- **Models:** Llama 2, Llama 3, Llama 3.1, Llama 4
- **Description:** Official tokenizer used by Llama-family models. Typically loaded through Hugging Face Transformers or Tokenizers.
- **Language:** Rust core with JavaScript bindings
- **Download:**
  - https://github.com/huggingface/tokenizers
  - https://huggingface.co/meta-llama

---

## Google

### Gemma Tokenizer

- **Provider:** Google
- **Models:** Gemma, Gemma 2, Gemma 3
- **Description:** SentencePiece-based tokenizer used by the Gemma family of models.
- **Language:** SentencePiece / Hugging Face
- **Download:**
  - https://huggingface.co/google
  - https://github.com/huggingface/tokenizers

---

## Mistral AI

### Mistral Tokenizer

- **Provider:** Mistral AI
- **Models:** Mistral 7B, Mixtral, Magistral, Mistral Large
- **Description:** Tokenizer used by Mistral-family models. Available through Hugging Face model repositories.
- **Language:** Hugging Face Tokenizers
- **Download:**
  - https://huggingface.co/mistralai
  - https://github.com/huggingface/tokenizers

---

## Alibaba

### Qwen Tokenizer

- **Provider:** Alibaba Cloud
- **Models:** Qwen 1, Qwen 2, Qwen 2.5, Qwen 3
- **Description:** Tokenizer used by the Qwen model family. Commonly used in open-source agent and coding benchmarks.
- **Language:** Hugging Face Tokenizers
- **Download:**
  - https://huggingface.co/Qwen
  - https://github.com/huggingface/tokenizers

---

## DeepSeek

### DeepSeek Tokenizer

- **Provider:** DeepSeek
- **Models:** DeepSeek-V3, DeepSeek-R1, DeepSeek-Coder
- **Description:** Tokenizer used by DeepSeek reasoning and coding models.
- **Language:** Hugging Face Tokenizers
- **Download:**
  - https://huggingface.co/deepseek-ai
  - https://github.com/huggingface/tokenizers

---

## xAI

### Grok Tokenizer

- **Provider:** xAI
- **Models:** Grok family
- **Description:** No standalone official tokenizer package is currently published. Token counting is typically performed through model-specific APIs or tokenizer assets distributed with released model checkpoints when available.
- **Language:** Varies by release
- **Download:**
  - https://x.ai

---

## Cohere

### Command Tokenizer

- **Provider:** Cohere
- **Models:** Command, Command R, Command R+
- **Description:** Tokenizer used by Cohere's Command model family. Available through released model artifacts and Hugging Face integrations.
- **Language:** Hugging Face Tokenizers
- **Download:**
  - https://huggingface.co/CohereLabs
  - https://github.com/huggingface/tokenizers

---

# Recommended Benchmark Set

For most compression benchmarking, these tokenizers provide coverage of the major commercial and open-weight ecosystems:

1. OpenAI — tiktoken
2. Anthropic — @anthropic-ai/tokenizer
3. Meta — Llama Tokenizer
4. Google — Gemma Tokenizer
5. Alibaba — Qwen Tokenizer
6. DeepSeek — DeepSeek Tokenizer

This set covers the majority of modern coding, reasoning, agent, and chat models used in production.