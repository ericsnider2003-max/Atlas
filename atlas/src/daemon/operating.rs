//! Doing things in apps, step by step (`operate`, 30 Sep 2026): the job is
//! started by a sentence, worked on the tick -- the screen is touched only
//! here, on the loop that owns it -- with each step's choice made by the
//! model on the crew, and checked before and after it's taken.

use super::*;
use crate::operate::{self, Action, Guard, Job, Target};

impl<'a> Daemon<'a> {
    /// "In Excel, make a new sheet called Budget": a job begun. The app is
    /// opened if it isn't; with none named, the window in front is the one.
    pub(super) fn start_operating(&mut self, said: &str) -> String {
        if self.llm.is_none() {
            return "Working an app step by step needs the language model, and it isn't running.".into();
        }
        if let Some(j) = &self.operating {
            return format!("I'm still working on \"{}\" -- say \"stop\" to end that first.", j.goal);
        }
        let apps: Vec<String> = self.cfg.apps.apps.keys().cloned().collect();
        let (app, goal) = operate::app_and_goal(said, &apps);
        let goal = if goal.trim().is_empty() { said.trim().to_string() } else { goal };
        let t = crate::store::now();
        let mut job = Job::new(&goal, &app, t);
        if app.is_empty() {
            match self.plat.active_window_id() {
                Ok(Some(w)) => job.win = Some(w.0),
                _ => return "Which app? Say it with the app's name -- \"in Excel, ...\".".into(),
            }
        }
        let where_ = if app.is_empty() { "the window in front".to_string() } else { app.clone() };
        self.log.info(&format!("operate: {goal} (in {where_})"));
        self.operating = Some(job);
        format!("Working on it in {where_}: {goal}. I'll go a step at a time and tell you when it's done -- say \"stop\" to end it.")
    }

    /// Stopped by "stop" or "stop everything": `true` when a job was going.
    pub(super) fn stop_operating(&mut self) -> Option<String> {
        let j = self.operating.take()?;
        self.finish_operating_step(crate::taskloop::Outcome::Failed(format!("Stopped app goal: {}.", j.goal)));
        Some(match j.steps.len() {
            0 => format!("Stopped \"{}\" before doing anything.", j.goal),
            n => format!("Stopped \"{}\" after {n} step{}: {}.", j.goal, if n == 1 { "" } else { "s" }, j.steps.last().cloned().unwrap_or_default()),
        })
    }

    /// Your answer to what a job asked. `None` when no job is waiting on you.
    pub(super) fn operate_answer(&mut self, said: &str) -> Option<String> {
        let waiting = self.operating.as_ref().map(|j| j.waiting_on_you).unwrap_or(false);
        if !waiting {
            return None;
        }
        self.session.pending = Pending::Nothing;
        let l = said.trim().to_lowercase();
        if l == "stop" || l.starts_with("stop ") || is_no(said) {
            return self.stop_operating();
        }
        let job = self.operating.as_mut()?;
        job.waiting_on_you = false;
        if let Some(name) = job.pending_press.take() {
            if is_yes(said) {
                job.allowed = Some(name.clone());
                // The step that was held, taken now.
                job.next = job.held.take();
                return Some(format!("Pressing \"{name}\"."));
            }
            return self.stop_operating();
        }
        job.last_result = format!("The user answered: {}", said.trim());
        Some("Got it -- carrying on.".into())
    }

    /// One pass of the job, on the tick.
    pub(super) fn operate_tick(&mut self, t: u64) -> Vec<String> {
        let had_job = self.operating.is_some();
        let out = self.operate_tick_inner(t);
        if had_job && self.operating.is_none() {
            // Success already releases the goal below. Every other terminal
            // path is a failure, including window/step/model limits.
            self.finish_operating_step(crate::taskloop::Outcome::Failed(out.last().cloned().unwrap_or_else(|| "The app job stopped without a result.".into())));
        }
        out
    }

    fn operate_tick_inner(&mut self, t: u64) -> Vec<String> {
        let mut out = Vec::new();
        let Some(mut job) = self.operating.take() else { return out };
        if job.thinking.is_some() || job.waiting_on_you || self.attention.is_paused() {
            self.operating = Some(job);
            return out;
        }
        // The window: found, or the app opened and waited for.
        let win = match job.win {
            Some(w) => crate::platform::WindowId(w),
            None => match self.window_for(&job.app) {
                Some(w) => {
                    job.win = Some(w.0);
                    w
                }
                None => {
                    if job.opened_at.is_none() {
                        let said = self.on_open_app(&Intent::OpenApp(job.app.clone()), &job.app.clone());
                        job.steps.push(format!("opened {}", job.app));
                        job.opened_at = Some(t);
                        self.log.info(&format!("operate: {said}"));
                    } else if t.saturating_sub(job.opened_at.unwrap_or(t)) > 20 {
                        out.push(format!("I opened {} but couldn't find its window, so I've stopped.", job.app));
                        return out;
                    }
                    self.operating = Some(job);
                    return out;
                }
            },
        };
        // Your hands are on the keyboard: the step waits for a gap, unless
        // the job is new (you've only just asked).
        let hands_off = self.plat.input_idle_secs().map(|s| s >= 2).unwrap_or(true);
        if !hands_off && !job.steps.is_empty() {
            self.operating = Some(job);
            return out;
        }
        // A step chosen: taken.
        if let Some((name, arg)) = job.next.take() {
            let view = job.view.clone().unwrap_or_else(|| operate::view_of_text(&[], (0, 0), ""));
            let action = match operate::read_call(&name, &arg) {
                Ok(a) => a,
                Err(why) => {
                    job.last_result = format!("That wasn't a step ({why}); call one of the tools with a number shown.");
                    self.operating = Some(job);
                    return out;
                }
            };
            match operate::guard(&action, &view, job.allowed.as_deref()) {
                Guard::Refuse(why) => {
                    job.last_result = format!("Not taken: {why}.");
                    self.operating = Some(job);
                    return out;
                }
                Guard::AskFirst(button) => {
                    let q = format!(
                        "For \"{}\", next I'd press \"{button}\" -- that can't be taken back. Go ahead?",
                        job.goal
                    );
                    job.pending_press = Some(button);
                    job.held = Some((name, arg));
                    job.waiting_on_you = true;
                    self.session.ask(&q);
                    out.push(q);
                    self.operating = Some(job);
                    return out;
                }
                Guard::Go => {}
            }
            match action {
                Action::Done(summary) => {
                    self.journal.record_at(Act::Upkeep, &format!("in {}: {}", job.app, job.goal), true, t);
                    out.push(if summary.trim().is_empty() { format!("Done: {}.", job.goal) } else { summary });
                    self.finish_operating_step(crate::taskloop::Outcome::Done(out.last().cloned().unwrap_or_default()));
                    return out;
                }
                Action::GiveUp(why) => {
                    out.push(format!("I couldn't do \"{}\": {why}", job.goal));
                    return out;
                }
                Action::Ask(q) => {
                    job.waiting_on_you = true;
                    self.session.ask(&q);
                    out.push(q);
                    self.operating = Some(job);
                    return out;
                }
                Action::Open(app) => {
                    let said = self.on_open_app(&Intent::OpenApp(app.clone()), &app);
                    job.steps.push(format!("opened {app}"));
                    job.last_result = said;
                    job.app = app;
                    job.win = None;
                    job.opened_at = Some(t);
                }
                other => {
                    let plain = other.plain(&view);
                    let result = self.take_step(win, &other, &view);
                    job.last_result = match &result {
                        Ok(()) => format!("{plain}."),
                        Err(why) => format!("tried to {plain} but {why}."),
                    };
                    job.steps.push(match result {
                        Ok(()) => plain,
                        Err(why) => format!("{plain} (failed: {why})"),
                    });
                    // The press you said yes to is used up.
                    if matches!(other, Action::Click(_)) {
                        job.allowed = None;
                    }
                }
            }
            self.operating = Some(job);
            return out;
        }
        // No step in hand: look, and ask for one.
        if job.steps.len() >= operate::MOST_STEPS {
            out.push(format!(
                "I stopped \"{}\" after {} steps without it being done. The last: {}.",
                job.goal,
                job.steps.len(),
                job.steps.last().cloned().unwrap_or_default()
            ));
            return out;
        }
        let view = match self.look_at(win) {
            Some(v) => v,
            None => {
                out.push(format!("I can't see anything in that window to work with, so I've stopped \"{}\".", job.goal));
                return out;
            }
        };
        let fp = view.fingerprint();
        if !job.steps.is_empty() && job.last_fingerprint == Some(fp) {
            job.unchanged += 1;
            job.last_result.push_str(" Nothing changed on screen after it.");
            if job.unchanged >= operate::MOST_UNCHANGED {
                out.push(format!(
                    "I've stopped \"{}\": the last {} steps changed nothing on screen. I got as far as: {}.",
                    job.goal,
                    job.unchanged,
                    job.steps.last().cloned().unwrap_or_default()
                ));
                return out;
            }
        } else {
            job.unchanged = 0;
        }
        job.last_fingerprint = Some(fp);
        let (system, user) = operate::prompt(&job, &view);
        job.view = Some(view);
        let Some(llm) = self.llm.clone() else {
            out.push("The language model stopped, so I've stopped working the app.".into());
            return out;
        };
        let tools = operate::tools();
        let work: crew::Work = Box::new(move |_c| {
            let req = crate::brain::ChatRequest {
                messages: vec![crate::brain::Msg::system(&system), crate::brain::Msg::user(&user)],
                tools: tools.clone(),
                max_tokens: crate::brain::FORCED_TOOL_TOKENS + 40,
                force_tool: true,
                stable_tools: 0,
                aside: true,
                stronger: false,
            };
            let r = llm.chat(&req, &mut |_| true).map_err(|e| e.to_string())?;
            let call = r.tool_calls.into_iter().next().or_else(|| crate::models::forced_call(&r.text, &tools));
            let call = call.ok_or_else(|| "the model didn't choose a step".to_string())?;
            let arg = call.arguments.get("arg").and_then(|a| a.as_str()).unwrap_or("").to_string();
            Ok(serde_json::json!({ "name": call.name, "arg": arg }).to_string())
        });
        job.thinking = self.hand_off_as("operate", t, work, Some(job.goal.clone()), SpeakPolicy::ViaWatcher);
        if job.thinking.is_none() {
            out.push("I have too much on to work the app right now -- ask me again in a minute.".into());
            return out;
        }
        self.operating = Some(job);
        out
    }

    /// The model's chosen step, back from the crew.
    pub(super) fn operate_news(&mut self, id: u64, ending: &crew::Ending) -> Option<String> {
        let had_job = self.operating.is_some();
        let said = self.operate_news_inner(id, ending);
        if had_job && self.operating.is_none() {
            self.finish_operating_step(crate::taskloop::Outcome::Failed(said.clone().unwrap_or_else(|| "The app job stopped without a result.".into())));
        }
        said
    }

    fn operate_news_inner(&mut self, id: u64, ending: &crew::Ending) -> Option<String> {
        let job = self.operating.as_mut()?;
        if job.thinking != Some(id) {
            return None;
        }
        job.thinking = None;
        match ending {
            crew::Ending::Done(Ok(json)) => {
                let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
                let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                let arg = v.get("arg").and_then(|n| n.as_str()).unwrap_or("").to_string();
                job.next = Some((name, arg));
                None
            }
            crew::Ending::Done(Err(why)) => {
                // Once more; twice running and the job ends.
                job.model_failures += 1;
                if job.model_failures >= 2 {
                    let goal = job.goal.clone();
                    self.operating = None;
                    return Some(format!("I stopped \"{goal}\": the model couldn't choose a step ({why})."));
                }
                None
            }
            _ => {
                let goal = job.goal.clone();
                self.operating = None;
                Some(format!("Stopped \"{goal}\"."))
            }
        }
    }

    /// The window of an app by its name: one of your apps, or the window in
    /// front when its program or title has the name in it.
    fn window_for(&self, app: &str) -> Option<crate::platform::WindowId> {
        if let Some((_, spec)) = self.cfg.apps.apps.iter().find(|(k, _)| k.eq_ignore_ascii_case(app)) {
            if let Ok(Some(w)) = self.plat.find_window(spec) {
                return Some(w);
            }
        }
        let a = app.to_lowercase();
        let front = self.plat.active_window().ok().flatten()?;
        if front.process.to_lowercase().contains(&a) || front.title.to_lowercase().contains(&a) {
            return self.plat.active_window_id().ok().flatten();
        }
        None
    }

    /// One look at the window: its controls, or, when it shows Windows
    /// none worth having, the words on its picture.
    fn look_at(&self, win: crate::platform::WindowId) -> Option<operate::View> {
        let title = self.plat.active_window().ok().flatten().map(|a| a.title).unwrap_or_default();
        if let Ok(Some(tree)) = self.plat.read_window(win) {
            let v = operate::view_of_tree(&tree, if title.is_empty() { &tree.name } else { &title });
            if operate::tree_is_usable(&v) {
                return Some(v);
            }
        }
        crate::heard!(self.plat.focus(win));
        let grab = self.plat.grab_window().ok().flatten()?;
        let origin = self.plat.rect_of(win).map(|r| (r.x, r.y)).unwrap_or((0, 0));
        let lines = self.plat.recognise_lines(&grab).ok()?;
        let v = operate::view_of_text(&lines, origin, &grab.title);
        (!v.targets.is_empty()).then_some(v)
    }

    /// Take one step on the screen.
    fn take_step(&self, win: crate::platform::WindowId, action: &Action, view: &operate::View) -> std::result::Result<(), String> {
        let target = |n: usize| view.targets.get(n.wrapping_sub(1)).cloned().ok_or_else(|| format!("there's no [{n}]"));
        let click_middle = |t: &Target| -> std::result::Result<(), String> {
            let (x, y) = t.middle().ok_or_else(|| "Windows didn't say where it is".to_string())?;
            crate::heard!(self.plat.focus(win));
            self.plat.click(x, y, crate::platform::Button::Left).map_err(|e| e.to_string())
        };
        match action {
            Action::Click(n) | Action::Choose(n) => {
                let t = target(*n)?;
                if let Target::Control { path, role, .. } = &t {
                    for act in operate::acts_for(action, *role) {
                        if let Ok(true) = self.plat.act_on(win, path, &act) {
                            return Ok(());
                        }
                    }
                }
                click_middle(&t)
            }
            Action::Type(n, text) => {
                let t = target(*n)?;
                if let Target::Control { path, role, .. } = &t {
                    if let Ok(true) = self.plat.act_on(win, path, &crate::uia::UiAct::SetValue(text.clone())) {
                        return Ok(());
                    }
                    let focused = matches!(self.plat.act_on(win, path, &crate::uia::UiAct::Focus), Ok(true));
                    if !focused {
                        click_middle(&t)?;
                    }
                    let _ = role;
                } else {
                    click_middle(&t)?;
                }
                crate::heard!(self.plat.press("ctrl+a"));
                self.plat.type_text(text).map_err(|e| e.to_string())
            }
            Action::Key(k) => {
                crate::heard!(self.plat.focus(win));
                self.plat.press(k).map_err(|e| e.to_string())
            }
            Action::Scroll(d) => {
                if let Ok(r) = self.plat.rect_of(win) {
                    crate::heard!(self.plat.move_cursor(r.x + r.width / 2, r.y + r.height / 2));
                }
                self.plat.scroll(0, *d).map_err(|e| e.to_string())
            }
            _ => Ok(()),
        }
    }
}
