![trchat2](https://user-images.githubusercontent.com/34670283/160282372-a048a12c-911a-40da-8dce-a737c9596055.png)

[![Version](https://img.shields.io/github/v/release/TrPlugins/TrChat?logo=VirusTotal&style=for-the-badge)](https://github.com/FlickerProjects/TrChat/releases)
[![Issues](https://img.shields.io/github/issues/TrPlugins/TrChat?logo=StackOverflow&style=for-the-badge)](https://github.com/FlickerProjects/TrChat/issues)
[![Last Commit](https://img.shields.io/github/last-commit/TrPlugins/TrChat?logo=ApacheRocketMQ&style=for-the-badge&color=1e90ff)](https://github.com/FlickerProjects/TrChat/commits/v2)
[![Downloads](https://img.shields.io/github/downloads/TrPlugins/TrChat/total?style=for-the-badge&logo=docusign)](https://github.com/FlickerProjects/TrChat/releases)
---

### 🔔 What's new in TrChat v2?
- **Optimized performance**
- **New Channel & Format System**
- **Better compatibility with other plugins**

---

### ⛏ API usage: 
```java
public class Demo implements Listener {
    
    @EventHandler
    private void e(TrChatEvent e) {
        e.getChannel(); // 获取聊天频道
        e.setCanceled(true); // 取消发送聊天
        e.setMessage("..."); // 改变聊天内容
    }   
}
```

---

### 🎃 PumpkinMC（实验性支持）

本分支（`pumpkin-experimental`）包含对 [PumpkinMC](https://pumpkinmc.org)（Rust 实现的
Minecraft 服务器）的实验性支持：`pumpkin/` 是一个独立的 Rust crate，以 WASM Component
插件形式在 Pumpkin 上提供 TrChat 的本地聊天核心（事件拦截、格式渲染、广播）。
详见 [pumpkin/README.md](pumpkin/README.md)。
