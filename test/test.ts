import { encodingForModel } from "js-tiktoken";
import { countTokens as countAnthropic } from "@anthropic-ai/tokenizer";
import { AutoTokenizer } from "@huggingface/transformers";

const openai = encodingForModel("gpt-4o");

const llama = await AutoTokenizer.from_pretrained(
  "meta-llama/Llama-3.1-8B-Instruct"
);

const gemma = await AutoTokenizer.from_pretrained(
  "google/gemma-3-12b-it"
);

const qwen = await AutoTokenizer.from_pretrained(
  "Qwen/Qwen3-32B"
);

console.log("OpenAI", openai.encode(text).length);
console.log("Anthropic", countAnthropic(text));
console.log("Llama", llama.encode(text).length);
console.log("Gemma", gemma.encode(text).length);
console.log("Qwen", qwen.encode(text).length);