from library.atlas import atlas_new, atlas_renewed_due
from library.book import book_due, book_fine, book_line, book_new, book_renewed_due, book_sample, book_titles
from library.comic import comic_new, comic_renewed_due
from library.common import Format, daily_fine, fine_cap, fine_for, format_name, formats, loan_days, renewal
from library.dvd import dvd_fine, dvd_new
from library.journal import journal_checkout, journal_new, journal_overdue, journal_renewed_due
from library.score import score_new, score_renewed_due
from library.student import student_charge, student_line, student_new


def test_hidden_cap():
    assert fine_cap() == 800
    assert fine_for(Format.PRINT, 100) == 800
    assert fine_for(Format.AUDIO, 30) == 300


def test_hidden_cap_used():
    assert book_fine(book_new(1, "a", 2000, 1, Format.PRINT), 0, 60) == 800
    assert dvd_fine(dvd_new(1, "d", 2000, 1, Format.AUDIO), 0, 74) == 600


def test_hidden_renewal():
    assert renewal(21, Format.PRINT, 0) == 31
    assert renewal(21, Format.PRINT, 1) == 31
    assert renewal(21, Format.PRINT, 2) == 21
    assert renewal(14, Format.EBOOK, 5) == 14
    assert renewal(7, Format.AUDIO, 0) == 14


def test_hidden_renewed_callers():
    assert book_renewed_due(book_new(1, "a", 2000, 1, Format.PRINT), 0, 0) == 31
    assert journal_renewed_due(journal_new(1, "j", 2000, 1, Format.EBOOK), 10, 1) == 31
    assert score_renewed_due(score_new(1, "s", 2000, 1, Format.PRINT), 0, 2) == 21


def test_hidden_renewed_rest():
    assert comic_renewed_due(comic_new(1, "c", 2000, 1, Format.AUDIO), 0, 3) == 14
    assert atlas_renewed_due(atlas_new(1, "a", 2000, 1, Format.PRINT), 5, 1) == 36


def test_hidden_microfilm():
    assert loan_days(Format.MICROFILM) == 7
    assert daily_fine(Format.MICROFILM) == 50
    assert format_name(Format.MICROFILM) == "microfilm"


def test_hidden_formats():
    assert formats() == [Format.PRINT, Format.EBOOK, Format.AUDIO, Format.MICROFILM]


def test_hidden_microfilm_used():
    assert book_line(book_new(7, "Atlas", 1999, 3, Format.MICROFILM)) == "Atlas (1999) [BOO-7] 3/3 microfilm"
    assert book_fine(book_new(1, "a", 2000, 1, Format.MICROFILM), 0, 10) == 150
    assert book_due(book_new(1, "a", 2000, 1, Format.MICROFILM), 3) == 10


def test_hidden_overdue():
    assert journal_overdue([journal_checkout(journal_new(1, "Quiet River", 2001, 2, Format.PRINT)), journal_new(2, "Alpha", 2002, 1, Format.PRINT), journal_checkout(journal_new(3, " Bright Atlas", 2003, 1, Format.EBOOK)), journal_checkout(journal_new(4, "Atlas Engine", 1990, 1, Format.AUDIO))], 0, 30) == ["atlas engine", "quiet river"]


def test_hidden_overdue_edges():
    assert journal_overdue([journal_checkout(journal_new(1, "a", 2000, 1, Format.PRINT))], 0, 21) == []
    assert journal_overdue([], 0, 99) == []


def test_hidden_unchanged():
    assert book_titles(book_sample()) == ["lost atlas", "northern voyage", "silent river"]
    assert fine_for(Format.PRINT, 10) == 250
    assert student_line(student_charge(student_new(3, "Ann", 1), 250)) == "Ann (tier 1): 0/3 loans, owes $2.50"
