import { test } from "node:test";
import assert from "node:assert/strict";
import { alumniNew, alumniReminder } from "../library/alumni";
import { bookCheckout, bookNew, bookSample, bookTitles } from "../library/book";
import { fineBlock, fineFor, fineNotice, maxLoans, titleKey } from "../library/common";
import { facultyCharge, facultyNew, facultyReminder } from "../library/faculty";
import { familyBorrow, familyCharge, familyNew } from "../library/family";
import { guestBorrow, guestCharge, guestNew } from "../library/guest";
import { staffCanTake, staffCharge, staffNew, staffReminder, staffSuspend } from "../library/staff";
import { studentCanBorrow, studentCharge, studentNew, studentReminder } from "../library/student";
import { thesisAvailable, thesisCheckout, thesisLine, thesisNew, thesisSample } from "../library/thesis";
import { zineNew, zineTitles } from "../library/zine";

test("hidden_title_key", () => {
  assert.equal(titleKey("The Hobbit"), "hobbit");
  assert.equal(titleKey("  the Hobbit "), "hobbit");
  assert.equal(titleKey("THE END"), "end");
  assert.equal(titleKey("Theory"), "theory");
  assert.equal(titleKey("Other Worlds"), "other worlds");
});

test("hidden_titles_used", () => {
  assert.deepEqual(bookTitles([bookNew(1, "The Lost River", 2000, 1, "print"), bookNew(2, "Archive", 2000, 1, "print"), bookNew(3, "the Garden", 2000, 1, "print")]), ["archive", "garden", "lost river"]);
  assert.deepEqual(zineTitles([zineNew(1, "The Zine", 2000, 1, "print"), zineNew(2, "Theme", 2000, 1, "print")]), ["theme", "zine"]);
});

test("hidden_block", () => {
  assert.equal(fineBlock(), 500);
  assert.equal(studentCanBorrow(studentCharge(studentNew(1, "a", 1), 500)), false);
  assert.equal(studentCanBorrow(studentCharge(studentNew(1, "a", 1), 499)), true);
});

test("hidden_block_used", () => {
  assert.equal(familyBorrow(familyCharge(familyNew(1, "a", 2), 700)).loans, 0);
  assert.equal(guestBorrow(guestCharge(guestNew(1, "a", 1), 120)).loans, 1);
});

test("hidden_fine_notice", () => {
  assert.equal(fineNotice("Bo", 1205), "Dear Bo, you owe $12.05.");
  assert.equal(fineNotice("Al", 7), "Dear Al, you owe $0.07.");
});

test("hidden_reminders", () => {
  assert.equal(studentReminder(studentCharge(studentNew(1, "Al", 1), 250)), "Dear Al, you owe $2.50.");
  assert.equal(staffReminder(staffCharge(staffNew(1, "Bea", 1), 1000)), "Dear Bea, you owe $10.00.");
  assert.equal(facultyReminder(facultyCharge(facultyNew(1, "Cy", 1), 5)), "Dear Cy, you owe $0.05.");
  assert.equal(alumniReminder(alumniNew(1, "Di", 1)), "Dear Di, you owe $0.00.");
});

test("hidden_thesis_advisor", () => {
  assert.equal(thesisNew(1, "On Rivers", 2010, 1, "print", "Dr. Lee").advisor, "Dr. Lee");
  assert.equal(thesisLine(thesisNew(7, "Atlas", 1999, 3, "audio", "Kim")), "Atlas (1999) [THE-7] 3/3 audio / Kim");
});

test("hidden_thesis_plain", () => {
  assert.equal(thesisLine(thesisNew(7, "Atlas", 1999, 3, "audio", "")), "Atlas (1999) [THE-7] 3/3 audio");
  assert.deepEqual(thesisSample().map((x) => x.advisor), ["", "", ""]);
  assert.equal(thesisAvailable(thesisCheckout(thesisNew(1, "a", 2000, 2, "print", "X"))), 1);
});

test("hidden_staff_can_take", () => {
  assert.equal(staffCanTake(staffNew(1, "a", 1), bookNew(1, "b", 2000, 1, "print")), true);
  assert.equal(staffCanTake(staffNew(1, "a", 1), bookCheckout(bookNew(1, "b", 2000, 1, "print"))), false);
  assert.equal(staffCanTake(staffSuspend(staffNew(1, "a", 1)), bookNew(1, "b", 2000, 1, "print")), false);
  assert.equal(staffCanTake(staffCharge(staffNew(1, "a", 1), 600), bookNew(1, "b", 2000, 1, "print")), false);
});

test("hidden_unchanged", () => {
  assert.deepEqual(bookTitles(bookSample()), ["lost atlas", "northern voyage", "silent river"]);
  assert.equal(fineFor("print", 10), 250);
  assert.equal(maxLoans(3), 10);
});
