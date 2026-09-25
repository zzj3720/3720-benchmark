import json, shlex
from tools.agents.pi_goal import CampaignGoalPi
class DeepSeek41Pi(CampaignGoalPi):
    async def install(self, environment):
        await super().install(environment)
        config = {"providers":{"deepseek":{"baseUrl":"https://api.deepseek.com","api":"openai-completions","apiKey":"DEEPSEEK_API_KEY","models":[{"id":"deepseek-v4.1-flash-expires-on-0910","reasoning":True,"input":["text"],"contextWindow":131072,"maxTokens":32768,"compat":{"supportsDeveloperRole":False,"supportsStore":False,"maxTokensField":"max_tokens"}}]}}}
        await self.exec_as_agent(environment,command="mkdir -p $HOME/.pi/agent; printf %s " + shlex.quote(json.dumps(config)) + " > $HOME/.pi/agent/models.json")
