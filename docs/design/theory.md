# The theory of Bossylights

Status: draft for review. Nothing here is built. Comment on the numbered questions at the end.

Peter Naur's point: a program is not its code, it is the theory in the heads of the people who build it. The theory lets you answer three things without reading code: what the program does, why each part is the way it is, and how it should respond to a change. This page is that theory, written down before any code so it can be argued with.

## The theory in one paragraph

Forty-four times a second the engine asks one question: what should every light be doing right now? The answer is computed fresh each time from three things: what is playing, on which layer, and what time it is. Nothing ever "sets a channel". Things that play contribute to the answer, layers decide who wins, and only the last step turns the answer into DMX bytes. Everything a person, a controller, a timeline or the autopilot does arrives as the same kind of command, so recording is writing commands down, a show is commands played back, and the autopilot is a program that issues commands from the music.

## Nine ideas

### 1. A frame is computed, never stored

- Each frame is a pure result: `frame = patch(merge(layers at time t))`.
- There is no long-lived "current DMX state" that parts of the program write into.
- Why: QLC+ functions write into shared universes, and its crashes and races live exactly there. A computed frame has no shared state to corrupt.
- What follows: stop, release and blackout are trivial (the thing just stops contributing), and preview is the same computation with the output switched off.

### 2. The engine thinks in lights, not in channels

- The unit is an **attribute of a fixture**: intensity, colour, position, beam, of "Mover L1".
- DMX channels and addresses appear in exactly one place, the patch, at the very end.
- Why: this is what makes "never remember a number" possible, lets a show move to another rig, and lets everything work with no hardware attached.
- Cost: a fixture with an odd channel needs a definition, not a hack. The escape hatch is a "raw channel" attribute, visible only in DMX control.

### 3. A look is a value

- A **look** is a partial answer: some attributes of some fixtures. "PARs amber at 80%."
- Looks are plain data. They can be stored, compared, blended, sent to an agent, shown in a preview.
- Palettes are named pieces of looks that other looks refer to.

### 4. Everything that moves is a source

- A **source** is a function: `source(time, clock, inputs) -> look`.
- A static look, a chaser, a movement effect, a pixel matrix, an audio-reactive script and a laser shape are all sources. The engine cannot tell them apart and does not need to.
- Why: QLC+ has ten function types with ten sets of rules. One shape means one way to start, stop, fade, layer, record and preview all of them.

### 5. Layers stack, and the top one that speaks wins

- Three layers, fixed, bottom to top: **Autopilot**, **Show**, **Live**.
- For each attribute of each fixture, the highest layer that has something to say wins, faded by its level.
- Release a control on the Live layer and whatever is underneath shows through again.
- This is the whole answer to "can I busk over a running show" and "does the autopilot fight me". It replaces HTP and LTP in the default vocabulary.

### 6. One clock

- One **clock** gives beat, bar, tempo and song position.
- It is fed by one of: tap, audio, MIDI clock, the DJ software, timecode.
- Sources read the clock. Launch-on-the-beat, tempo sync and timecode chase are all the same thing: which feed drives the clock.

### 7. Everything arrives as a command

- "Start this source on the Live layer." "Set master to 85%." "Release."
- The screen, a Launchpad, OSC, an AI agent over MCP, the timeline and the autopilot all send the same commands through the same door.
- **Recording** is writing the commands down with their time.
- **A show** is a list of commands played back against the clock.
- **The autopilot** is a program that listens to the music and issues commands.
- Why this matters most: live, recorded and automatic are not three features. They are three writers of one stream, which is why they can be switched and combined.

### 8. Two kinds of state

- **The project** is what you made: rig, looks, sources, surface, shows. It is saved, versioned and undoable.
- **The performance** is what is happening: what runs, levels, the clock. It is never saved by accident, and it can be recorded.
- Editing changes the project. Playing changes the performance. Undo belongs to the project.

### 9. The real-time core is small and dumb

- The part that runs 44 times a second only evaluates sources, merges layers, patches and sends.
- It takes no locks, allocates nothing and never waits for anything.
- Everything clever (song analysis, the autopilot's choices, files, the UI, agents) lives outside it and talks to it through commands in and snapshots out.
- A frozen UI or a crashed analysis cannot stop the lights.

## Three walks through the system

**You press a pad.**

1. The surface turns the press into a command: start "Sunset" on Live.
2. The command goes through the one door.
3. The mixer adds the source to the Live layer, on the next beat if launch-on-the-beat is on.
4. From the next frame on, the answer to "what should every light do" includes Sunset on top.
5. The state snapshot changes, so the pad on screen and on the Launchpad both light up. Nobody told the Launchpad anything.

**You record a busk and play it back.**

1. Recording is on. Every command you issue is written down with its clock time.
2. That list is a show. You can trim it, move it, quantize it.
3. Playing it sends the same commands to the Show layer.
4. While it plays you grab a fader. That is a Live command, Live is on top, so you win. Let go and the show is back.

**The autopilot yields to your hand.**

1. The autopilot hears the build-up coming and starts a rising look on the Autopilot layer.
2. You hold Strobe. Live speaks for the PARs, so for the PARs the autopilot is covered.
3. The movers were not touched, so the autopilot still moves them.
4. You release. The PARs return to the autopilot without a cut.

## The test of the theory: what is easy, what is hard

| Change | Under this theory |
|---|---|
| A new output protocol | One new adapter behind Output. Nothing else knows |
| A new kind of effect | One new source. It can be layered, recorded and previewed on day one |
| A new controller | A profile in Surface. It mirrors the screen for free |
| Laser shapes | A source whose look speaks "laser frame" attributes, and one output adapter |
| A second UI, or an agent | A new client of the one door |
| "Just set channel 37 to 255" | Deliberately awkward. It goes through a raw-channel attribute |
| More than three layers | A real change to idea 5. Needs a reason |
| A source that needs another source's output | Not possible directly. Compose them into one source instead |

## What the theory says no to

- No part of the program writes DMX values except Output.
- No feature gets its own private way to start, stop or fade things.
- No UI code holds truth. The UI shows snapshots and sends commands.
- No smart code inside the frame loop.

## Questions for review

1. **Top layer wins, for every attribute.** Or should intensity add up inside a layer, so two washes together are brighter (the QLC+ "highest wins" habit)?
2. **Three fixed layers** (Autopilot, Show, Live). Or should layers be something the user can add?
3. **Recording stores commands**, not DMX values. Replays stay editable and follow rig changes, but a replay is only as exact as the sources are repeatable. Good trade?
4. **Lights, not channels.** Are you fine with raw channels living only in the DMX control screen?
5. **One clock.** When the DJ software and the audio input disagree about the beat, who wins: the one you picked, or the most confident?
6. **Song Director** is then just an editor for a show whose blocks follow song sections. Does that match how you think of it?
7. Is anything you rely on in QLC+ impossible to say in these nine ideas?
