use std::process::Command;

use serde_json::Value;

struct Oracle {
    alarm_sequence: u64,
}

impl Oracle {
    fn run() -> Result<(), String> {
        let mut oracle = Self { alarm_sequence: 0 };
        let initial = oracle.show()?;
        if initial["shift"]["status"] == "not_started" {
            oracle.call(&["start"])?;
        }

        while oracle.show()?["campaign"]["score"].as_i64().unwrap_or(0) < 70 {
            oracle.deliver_soup()?;
        }

        let submitted = oracle.call(&["submit"])?;
        let score = submitted["data"]["score"]
            .as_i64()
            .ok_or_else(|| "submit response omitted score".to_owned())?;
        if score < 70 {
            return Err(format!(
                "oracle completed without establishing a positive final score: {score}"
            ));
        }
        println!("kitchen Oracle score: {score}");
        Ok(())
    }

    fn deliver_soup(&mut self) -> Result<(), String> {
        let state = self.wait_for_order()?;
        if state["orders"][0]["cooking_step"] != "Pot" {
            return Err("active order is not a pot recipe".to_owned());
        }
        let requirement = state["orders"][0]["requirements"]
            .as_array()
            .ok_or_else(|| "active order omitted requirements".to_owned())?
            .iter()
            .filter(|requirement| requirement["kind"] == "ingredient")
            .find(|requirement| requirement["quantity"].as_u64() == Some(3))
            .ok_or_else(|| "active soup did not require three matching ingredients".to_owned())?;
        let ingredient = requirement["id"]
            .as_str()
            .ok_or_else(|| "soup requirement omitted ingredient id".to_owned())?
            .to_owned();
        let source = self.object_id(|object| object["supply"] == ingredient)?;
        let boards = self.reachable_destinations("workstation")?;
        if boards.len() < 2 {
            return Err("soup kitchen has fewer than two chopping boards".to_owned());
        }
        let pot = self.reachable_object(|object| {
            object["item"]["kind"] == "container"
                && object["item"]["cooking"]["step"] == "Pot"
                && object["item"]["contents"]
                    .as_array()
                    .is_some_and(Vec::is_empty)
        })?;
        let bin = self
            .reachable_destination("bin")
            .map_err(|error| format!("bin: {error}"))?;
        let delivery = self
            .reachable_destination("delivery")
            .map_err(|error| format!("delivery: {error}"))?;

        self.take_to(&source, &boards[0])?;
        let first_due = self.work_due(&boards[0])?;
        self.call(&["switch"])?;
        self.take_to(&source, &boards[1])?;
        let second_due = self.work_due(&boards[1])?;
        self.wait_until(first_due.max(second_due))?;

        self.call(&["interact", &boards[1]])?;
        self.go_near(&pot)?;
        self.call(&["interact", &pot])?;
        self.start_travel_if_needed(&bin)?;
        self.call(&["switch"])?;

        self.call(&["interact", &boards[0]])?;
        self.go_near(&pot)?;
        self.call(&["interact", &pot])?;
        self.take_to(&source, &boards[0])?;
        let third_due = self.work_due(&boards[0])?;
        self.wait_until(third_due)?;
        self.call(&["interact", &boards[0]])?;
        self.go_near(&pot)?;
        self.call(&["interact", &pot])?;

        let ready_ms = self.pot_ready_ms(&pot)?;
        let plate = self.wait_for_clean_plate()?;
        self.go_near(&plate)?;
        self.call(&["interact", &plate])?;
        self.go_near(&pot)?;
        let elapsed_ms = self.show()?["shift"]["elapsed_ms"].as_u64().unwrap_or(0);
        if elapsed_ms < ready_ms {
            self.wait_until(ready_ms)?;
        }
        self.call(&["interact", &pot])?;
        self.go_near(&delivery)?;
        self.call(&["interact", &delivery])?;
        Ok(())
    }

    fn wait_for_order(&mut self) -> Result<Value, String> {
        loop {
            let state = self.show()?;
            if state["shift"]["status"] == "complete" {
                return Err("shift ended before the Oracle reached its score target".to_owned());
            }
            if state["orders"]
                .as_array()
                .is_some_and(|orders| !orders.is_empty())
            {
                return Ok(state);
            }
            self.wait_millis(1_000)?;
        }
    }

    fn wait_for_clean_plate(&mut self) -> Result<String, String> {
        loop {
            if let Ok(plate) = self.reachable_object(|object| {
                object["item"]["kind"] == "plate"
                    || object["plate_count"]
                        .as_u64()
                        .is_some_and(|count| count > 0)
            }) {
                return Ok(plate);
            }
            let state = self.show()?;
            if state["shift"]["status"] == "complete" {
                return Err("shift ended while waiting for a clean plate".to_owned());
            }
            if let Some(dirty) = state["map"]["objects"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|object| {
                    object["kind"] == "plate_return"
                        && object["dirty_plate_count"]
                            .as_u64()
                            .is_some_and(|count| count > 0)
                })
                .and_then(|object| object["id"].as_str())
            {
                let dirty = dirty.to_owned();
                let sink = self
                    .reachable_destination("sink")
                    .map_err(|error| format!("sink: {error}"))?;
                self.take_to(&dirty, &sink)?;
                let washed_due = self.work_due(&sink)?;
                self.wait_until(washed_due)?;
                self.call(&["stop"])?;
                continue;
            }
            self.wait_millis(1_000)?;
        }
    }

    fn work_due(&self, target: &str) -> Result<u64, String> {
        self.call(&["work", target])?["data"]["expected_done_ms"]
            .as_u64()
            .ok_or_else(|| "work response omitted expected_done_ms".to_owned())
    }

    fn pot_ready_ms(&self, pot: &str) -> Result<u64, String> {
        let state = self.show()?;
        let elapsed_ms = state["shift"]["elapsed_ms"]
            .as_u64()
            .ok_or_else(|| "state omitted elapsed_ms".to_owned())?;
        let cooking = state["map"]["objects"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|object| object["id"].as_str() == Some(pot))
            .and_then(|object| object["item"]["cooking"].as_object())
            .ok_or_else(|| "pot omitted cooking state".to_owned())?;
        let duration_ms = cooking["duration_ms"]
            .as_u64()
            .ok_or_else(|| "pot omitted cooking duration".to_owned())?;
        let progress_ms = cooking["progress_ms"]
            .as_u64()
            .ok_or_else(|| "pot omitted cooking progress".to_owned())?;
        Ok(elapsed_ms + duration_ms.saturating_sub(progress_ms))
    }

    fn start_travel_if_needed(&self, target: &str) -> Result<(), String> {
        let state = self.show()?;
        let destination = state["destinations"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|destination| destination["target"].as_str() == Some(target))
            .ok_or_else(|| format!("{target} is not currently reachable"))?;
        if destination["travel_ms"].as_u64().unwrap_or(0) > 0 {
            self.call(&["go", target])?;
        }
        Ok(())
    }

    fn take_to(&mut self, source: &str, destination: &str) -> Result<(), String> {
        self.go_near(source)?;
        self.call(&["interact", source])?;
        self.go_near(destination)?;
        self.call(&["interact", destination])?;
        Ok(())
    }

    fn reachable_object(&self, predicate: impl Fn(&Value) -> bool) -> Result<String, String> {
        let state = self.show()?;
        let destinations = state["destinations"]
            .as_array()
            .ok_or_else(|| "state omitted destinations".to_owned())?;
        state["map"]["objects"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|object| predicate(object))
            .find_map(|object| {
                let id = object["id"].as_str()?;
                destinations
                    .iter()
                    .any(|destination| destination["target"].as_str() == Some(id))
                    .then(|| id.to_owned())
            })
            .ok_or_else(|| "no matching authored object is currently reachable".to_owned())
    }

    fn reachable_destination(&self, kind: &str) -> Result<String, String> {
        self.reachable_destinations(kind)?
            .into_iter()
            .next()
            .ok_or_else(|| format!("no {kind} destination is currently reachable"))
    }

    fn reachable_destinations(&self, kind: &str) -> Result<Vec<String>, String> {
        let destinations = self.show()?["destinations"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|destination| destination["kind"].as_str() == Some(kind))
            .filter_map(|destination| destination["target"].as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        (!destinations.is_empty())
            .then_some(destinations)
            .ok_or_else(|| format!("no {kind} destination is currently reachable"))
    }

    fn object_id(&self, predicate: impl Fn(&Value) -> bool) -> Result<String, String> {
        self.show()?["map"]["objects"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|object| predicate(object))
            .and_then(|object| object["id"].as_str())
            .map(str::to_owned)
            .ok_or_else(|| "matching authored object is missing".to_owned())
    }

    fn go_near(&mut self, target: &str) -> Result<(), String> {
        let state = self.show()?;
        let Some(destination) = state["destinations"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|destination| destination["target"].as_str() == Some(target))
        else {
            return Err(format!("{target} is not currently reachable"));
        };
        if destination["travel_ms"].as_u64() == Some(0) {
            return Ok(());
        }
        let response = self.call(&["go", target])?;
        if response["data"]["expected_arrival_ms"].as_u64().is_none() {
            return Err("go response omitted expected_arrival_ms".to_owned());
        }
        loop {
            self.call(&["wait"])?;
            let state = self.show()?;
            let travelling = state["chefs"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|chef| chef["active"] == true)
                .is_some_and(|chef| chef["travel"].is_object());
            if !travelling {
                return Ok(());
            }
        }
    }

    fn wait_until(&mut self, due_ms: u64) -> Result<(), String> {
        let elapsed = self.show()?["shift"]["elapsed_ms"]
            .as_u64()
            .ok_or_else(|| "state omitted elapsed_ms".to_owned())?;
        self.wait_millis(due_ms.saturating_sub(elapsed).max(1))
    }

    fn wait_millis(&mut self, milliseconds: u64) -> Result<(), String> {
        self.alarm_sequence += 1;
        let id = format!("oracle-{}", self.alarm_sequence);
        let seconds = format!("{:.3}", milliseconds as f64 / 1_000.0);
        self.call(&["alarm", &id, &seconds, "oracle wait"])?;
        self.call(&["wait"])?;
        Ok(())
    }

    fn show(&self) -> Result<Value, String> {
        Ok(self.call(&["show"])?["data"].clone())
    }

    fn call(&self, arguments: &[&str]) -> Result<Value, String> {
        let response = self.call_allow_failure(arguments)?;
        if response["ok"] == true {
            Ok(response)
        } else {
            Err(format!(
                "kitchen {} failed: {}",
                arguments.join(" "),
                response["error"]["message"]
                    .as_str()
                    .unwrap_or("unknown error")
            ))
        }
    }

    fn call_allow_failure(&self, arguments: &[&str]) -> Result<Value, String> {
        let output = Command::new("kitchen")
            .args(arguments)
            .output()
            .map_err(|error| format!("could not run kitchen client: {error}"))?;
        serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("kitchen client returned invalid JSON: {error}"))
    }
}

fn main() {
    if let Err(error) = Oracle::run() {
        eprintln!("kitchen Oracle failed: {error}");
        std::process::exit(1);
    }
}
