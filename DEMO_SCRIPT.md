# Demo script — "The Interns Who Taught the Drive-Thru to Code"

A live-slides demo talk. Nothing is pre-made: every visual on screen is drawn by Luna from
what the presenter says, in real time. Runs about **four minutes**.

**Before you start**

```bash
TALK_TERMS="LeetCode, McFlurry, McNugget, drive-thru, pull request, dynamic programming" ./demo.sh
```

Read the **Say** lines out loud, roughly as written — they are phrased to trigger the visual
next to them. Pause about two seconds at each `⏸`; that is where the board catches up.

---

## 1 · Meet the interns

> **Say:** "Good morning, and thanks for making time for the incident review. Before we get into
> what broke, let me introduce the two people at the center of this." ⏸
>
> "There are two interns on the ordering team. First, James — a summer software intern on the
> drive-thru chatbot. Second, Marcus — a summer software intern on the same team, and James's
> code reviewer." ⏸⏸

**On screen:** a text card with the two names and roles.

> **Say:** "This is James, a summer software intern in a McDonald's uniform." ⏸⏸
>
> "And this is Marcus, a summer software intern in a McDonald's uniform." ⏸⏸

**On screen:** two portraits, drawn on the spot (~2 s each — the library has no stock photo of
either of them, so they get generated).

> **Say:** "Let's move on." ⏸⏸

---

## 2 · The situation

> **Say:** "Here is the situation. There are three things you need to know." ⏸
>
> "First, our drive-thru chatbot takes the customer's order and sends it to the kitchen." ⏸
> "Second, James was asked to make the chatbot friendlier and more helpful." ⏸
> "Third, Marcus approved the change, and it shipped straight to production." ⏸⏸
>
> "Let's move on." ⏸⏸

**On screen:** a three-point text card, one line appearing per sentence.

---

## 3 · What the code change actually did

> **Say:** "So how is the ordering system supposed to work? It has four steps." ⏸
>
> "First, the customer speaks at the drive-thru. Then the chatbot parses the order. Next, the
> kitchen display prints a ticket. And finally, the customer pays at the window." ⏸⏸

**On screen:** a four-step flow diagram, growing one box per sentence.

> **Say:** "Now here is what James actually shipped." ⏸
> "Call that second step *solve the interview question* instead." ⏸⏸

**On screen:** the second box relabels itself in place — the diagram does not redraw. **This is
the moment of the demo.** Let it land before you keep going.

> **Say:** "The chatbot stopped parsing orders, and started solving LeetCode problems." ⏸
>
> "Let's move on." ⏸⏸

---

## 4 · Timeline

> **Say:** "Here is the timeline of what happened." ⏸
>
> "On Monday the ninth, James opened the pull request." ⏸
> "On Tuesday the tenth, Marcus approved it in four minutes." ⏸
> "On Wednesday the eleventh, the change went out to every restaurant." ⏸
> "On Thursday the twelfth, a customer asked for a McFlurry and got a dynamic programming
> solution." ⏸
> "On Friday the thirteenth, we rolled the change back." ⏸⏸
>
> "Let's move on." ⏸⏸

**On screen:** a dated timeline, one event per sentence. The dates are what make it render as a
timeline rather than a flow — keep saying them.

---

## 5 · What it cost

> **Say:** "Now let's talk about what it cost us. Here is daily revenue, in millions of dollars." ⏸
>
> "On Monday we made twelve million. On Tuesday twelve million. On Wednesday eleven million.
> On Thursday four million. And on Friday two million." ⏸⏸

**On screen:** a bar chart, one bar per number.

> **Say:** "Sorry — let's correct Thursday from four to three." ⏸⏸

**On screen:** that one bar changes. Nothing else moves.

> **Say:** "And on Saturday we recovered to six million." ⏸⏸
>
> **Say:** "Show that as a line chart." ⏸⏸
>
> **Say:** "Let's move on." ⏸⏸

> **Say:** "So where did the money actually go? Sixty percent was abandoned orders, thirty percent
> was refunds, and ten percent was the compute bill for solving LeetCode problems." ⏸⏸

**On screen:** a pie chart. (Say "sixty percent", never "most of it" — only numbers you actually
say get charted.)

> **Say:** "Let's move on." ⏸⏸

> **Say:** "One number to leave you with. Daily revenue dropped from twelve million dollars to two
> million dollars." ⏸⏸

**On screen:** a single big before → after figure.

---

## 6 · Close

> **Say:** "Let's move on." ⏸⏸
>
> "There are two lessons here. First, never let an intern approve another intern's pull request.
> Second, a chatbot that is too helpful is still an outage." ⏸⏸
>
> "Thanks, everyone. James and Marcus are now on the fries station." ⏸

---

## Presenter notes

- **Say the numbers.** "Sixty percent", not "most". A value that is not spoken is not charted.
- **"Let's move on" is the page break.** It clears the board, and only fires once every six
  seconds — don't stack two in a row.
- **Pause at the end of a sentence.** Text cards in particular wait for a finished sentence
  before they draw; the board settles about two seconds behind you.
- **Ad-libbing is fine and is worth doing** — correcting a number or renaming a step off-script is
  the most convincing thing you can do in this demo. Section 3 and the Thursday correction in
  section 5 are the two places to improvise.

**Known soft spots on this machine**

- No logo or icon library is configured (`LS_ASSETS` is unset), so a McDonald's logo would come up
  as a plain name card. The script avoids asking for one.
- The two intern portraits are generated, not looked up. If generation is slow or the wording
  drifts, they are silently skipped — the text card in section 1 carries the intro on its own, so
  the talk does not break.
