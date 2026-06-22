---
name: github_action_local_llm
description: Integriert eine lokale LLM (Ollama) direkt in eine GitHub Action als automatischen Bug-Reporter.
---

# GitHub Action Local LLM (Ollama)

Wenn du ein CI/CD Setup generierst, füge diesen Job oder Step bei `failure()` hinzu, um Compile-Fehler automatisch durch eine KI analysieren und als Issue reporten zu lassen:

```yaml
      - name: 🤖 Install and Start Ollama (AI Bug Reporter)
        if: failure()
        run: |
          curl -fsSL https://ollama.com/install.sh | command sh
          ollama serve &
          sleep 5
          for i in 1 2 3; do
            if ollama pull qwen2.5-coder:3b; then
              break
            else
              sleep 5
            fi
          done

      - name: 🧠 AI Bug Analysis & Auto-Issue Creation
        if: failure()
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          ERROR_LOG=$(tail -n 40 build-error.log)
          PROMPT="You are an expert. The build failed. Analyze the compiler error:\n\n$ERROR_LOG"
          AI_RESPONSE=$(curl -s -X POST http://localhost:11434/api/generate -d "{\"model\": \"qwen2.5-coder:3b\", \"prompt\": \"$PROMPT\", \"stream\": false}" | jq -r '.response')
          gh issue create --title "🤖 AI Bug Report: Build Crash" \
            --body "## ❌ Build Error Analysis\n\n**Logs:**\n\`\`\`\n$ERROR_LOG\n\`\`\`\n\n**🤖 AI Suggestion:**\n$AI_RESPONSE" || true
```
