import { test } from "node:test";
import assert from "node:assert/strict";
import { atlasNew, atlasRenewedDue } from "../library/atlas";
import { bookDue, bookFine, bookLine, bookNew, bookRenewedDue, bookSample, bookTitles } from "../library/book";
import { comicNew, comicRenewedDue } from "../library/comic";
import { dailyFine, fineCap, fineFor, formatName, formats, loanDays, renewal } from "../library/common";
import { dvdFine, dvdNew } from "../library/dvd";
import { journalCheckout, journalNew, journalOverdue, journalRenewedDue } from "../library/journal";
import { scoreNew, scoreRenewedDue } from "../library/score";
import { studentCharge, studentLine, studentNew } from "../library/student";

test("hidden_cap", () => {
  assert.equal(fineCap(), 800);
  assert.equal(fineFor("print", 100), 800);
  assert.equal(fineFor("audio", 30), 300);
});

test("hidden_cap_used", () => {
  assert.equal(bookFine(bookNew(1, "a", 2000, 1, "print"), 0, 60), 800);
  assert.equal(dvdFine(dvdNew(1, "d", 2000, 1, "audio"), 0, 74), 600);
});

test("hidden_renewal", () => {
  assert.equal(renewal(21, "print", 0), 31);
  assert.equal(renewal(21, "print", 1), 31);
  assert.equal(renewal(21, "print", 2), 21);
  assert.equal(renewal(14, "ebook", 5), 14);
  assert.equal(renewal(7, "audio", 0), 14);
});

test("hidden_renewed_callers", () => {
  assert.equal(bookRenewedDue(bookNew(1, "a", 2000, 1, "print"), 0, 0), 31);
  assert.equal(journalRenewedDue(journalNew(1, "j", 2000, 1, "ebook"), 10, 1), 31);
  assert.equal(scoreRenewedDue(scoreNew(1, "s", 2000, 1, "print"), 0, 2), 21);
});

test("hidden_renewed_rest", () => {
  assert.equal(comicRenewedDue(comicNew(1, "c", 2000, 1, "audio"), 0, 3), 14);
  assert.equal(atlasRenewedDue(atlasNew(1, "a", 2000, 1, "print"), 5, 1), 36);
});

test("hidden_microfilm", () => {
  assert.equal(loanDays("microfilm"), 7);
  assert.equal(dailyFine("microfilm"), 50);
  assert.equal(formatName("microfilm"), "microfilm");
});

test("hidden_formats", () => {
  assert.deepEqual(formats(), ["print", "ebook", "audio", "microfilm"]);
});

test("hidden_microfilm_used", () => {
  assert.equal(bookLine(bookNew(7, "Atlas", 1999, 3, "microfilm")), "Atlas (1999) [BOO-7] 3/3 microfilm");
  assert.equal(bookFine(bookNew(1, "a", 2000, 1, "microfilm"), 0, 10), 150);
  assert.equal(bookDue(bookNew(1, "a", 2000, 1, "microfilm"), 3), 10);
});

test("hidden_overdue", () => {
  assert.deepEqual(journalOverdue([journalCheckout(journalNew(1, "Quiet River", 2001, 2, "print")), journalNew(2, "Alpha", 2002, 1, "print"), journalCheckout(journalNew(3, " Bright Atlas", 2003, 1, "ebook")), journalCheckout(journalNew(4, "Atlas Engine", 1990, 1, "audio"))], 0, 30), ["atlas engine", "quiet river"]);
});

test("hidden_overdue_edges", () => {
  assert.deepEqual(journalOverdue([journalCheckout(journalNew(1, "a", 2000, 1, "print"))], 0, 21), []);
  assert.deepEqual(journalOverdue([], 0, 99), []);
});

test("hidden_unchanged", () => {
  assert.deepEqual(bookTitles(bookSample()), ["lost atlas", "northern voyage", "silent river"]);
  assert.equal(fineFor("print", 10), 250);
  assert.equal(studentLine(studentCharge(studentNew(3, "Ann", 1), 250)), "Ann (tier 1): 0/3 loans, owes $2.50");
});
