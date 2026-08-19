//! 工作区脚手架：agent 指令文档 + skills（loomgui-editor 围栏规则 + loom CLI 手册），
//! 从模板覆盖拷入。CLI `loom init` 与 GUI「新建工作区」共用（模板只此一份，防两端
//! 口径漂移）。模板文件在 crate 的 templates/ 下（include_str! 嵌进二进制——CLI 单
//! exe 分发）。

use std::path::Path;

/// 按 agent 类型写脚手架：`claude` 落 `CLAUDE.md` + `.claude/skills/`，
/// `agents` 落 `AGENTS.md` + `.agents/skills/`（AGENTS.md 约定的 agent 通用）。
/// 每个落点三件：指令文档（`{{SKILLS_DIR}}` 占位符替换）+ loomgui-editor skill +
/// loom CLI skill。覆盖拷入，不碰 workspace.json 和源文件。
pub fn write_agent_scaffold(root: &Path, agents: &[String]) -> Result<(), String> {
    if agents.is_empty() {
        return Err("no agent kind selected (expected `claude` and/or `agents`)".to_string());
    }
    let doc_tpl = include_str!("../templates/workspace-agent.md");
    let skill_md = include_str!("../templates/skill/SKILL.md");
    let loom_skill_md = include_str!("../templates/loom-skill/SKILL.md");
    for agent in agents {
        let (doc_name, skills_dir) = match agent.as_str() {
            "claude" => ("CLAUDE.md", ".claude/skills"),
            "agents" => ("AGENTS.md", ".agents/skills"),
            other => return Err(format!("unknown agent kind: {other}")),
        };
        let doc = doc_tpl.replace("{{SKILLS_DIR}}", skills_dir);
        let doc_path = root.join(doc_name);
        std::fs::write(&doc_path, doc).map_err(|e| format!("write {doc_name}: {e}"))?;
        for (dir, content) in [("loomgui-editor", skill_md), ("loom", loom_skill_md)] {
            let skill_dir = root.join(skills_dir).join(dir);
            std::fs::create_dir_all(&skill_dir).map_err(|e| format!("create skill dir: {e}"))?;
            std::fs::write(skill_dir.join("SKILL.md"), content)
                .map_err(|e| format!("write SKILL.md: {e}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 脚手架产物契约：指令文档占位符替换 + 双 skill（editor/loom）落位。
    #[test]
    fn scaffold_writes_doc_and_skills_per_agent() {
        let tmp = std::env::temp_dir().join(format!("loom_scaffold_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        write_agent_scaffold(&tmp, &["claude".to_string(), "agents".to_string()]).unwrap();

        for (doc, skills) in [
            ("CLAUDE.md", ".claude/skills"),
            ("AGENTS.md", ".agents/skills"),
        ] {
            let text = std::fs::read_to_string(tmp.join(doc)).unwrap();
            assert!(!text.contains("{{SKILLS_DIR}}"), "{doc} 占位符须替换");
            assert!(text.contains(skills), "{doc} 应指向 {skills}");
            assert!(
                tmp.join(skills).join("loomgui-editor/SKILL.md").exists(),
                "{doc} 的 editor skill 须落位"
            );
            let loom = std::fs::read_to_string(tmp.join(skills).join("loom/SKILL.md")).unwrap();
            assert!(loom.contains("loom check"), "loom skill 须含命令面");
        }
        // 未知 agent kind 拒绝。
        assert!(write_agent_scaffold(&tmp, &["vscode".to_string()]).is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
