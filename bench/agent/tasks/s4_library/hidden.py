from library.alumni import alumni_new, alumni_reminder
from library.book import book_checkout, book_new, book_sample, book_titles
from library.common import Format, fine_block, fine_for, fine_notice, max_loans, title_key
from library.faculty import faculty_charge, faculty_new, faculty_reminder
from library.family import family_borrow, family_charge, family_new
from library.guest import guest_borrow, guest_charge, guest_new
from library.staff import staff_can_take, staff_charge, staff_new, staff_reminder, staff_suspend
from library.student import student_can_borrow, student_charge, student_new, student_reminder
from library.thesis import thesis_available, thesis_checkout, thesis_line, thesis_new, thesis_sample
from library.zine import zine_new, zine_titles


def test_hidden_title_key():
    assert title_key("The Hobbit") == "hobbit"
    assert title_key("  the Hobbit ") == "hobbit"
    assert title_key("THE END") == "end"
    assert title_key("Theory") == "theory"
    assert title_key("Other Worlds") == "other worlds"


def test_hidden_titles_used():
    assert book_titles([book_new(1, "The Lost River", 2000, 1, Format.PRINT), book_new(2, "Archive", 2000, 1, Format.PRINT), book_new(3, "the Garden", 2000, 1, Format.PRINT)]) == ["archive", "garden", "lost river"]
    assert zine_titles([zine_new(1, "The Zine", 2000, 1, Format.PRINT), zine_new(2, "Theme", 2000, 1, Format.PRINT)]) == ["theme", "zine"]


def test_hidden_block():
    assert fine_block() == 500
    assert student_can_borrow(student_charge(student_new(1, "a", 1), 500)) == False
    assert student_can_borrow(student_charge(student_new(1, "a", 1), 499)) == True


def test_hidden_block_used():
    assert family_borrow(family_charge(family_new(1, "a", 2), 700)).loans == 0
    assert guest_borrow(guest_charge(guest_new(1, "a", 1), 120)).loans == 1


def test_hidden_fine_notice():
    assert fine_notice("Bo", 1205) == "Dear Bo, you owe $12.05."
    assert fine_notice("Al", 7) == "Dear Al, you owe $0.07."


def test_hidden_reminders():
    assert student_reminder(student_charge(student_new(1, "Al", 1), 250)) == "Dear Al, you owe $2.50."
    assert staff_reminder(staff_charge(staff_new(1, "Bea", 1), 1000)) == "Dear Bea, you owe $10.00."
    assert faculty_reminder(faculty_charge(faculty_new(1, "Cy", 1), 5)) == "Dear Cy, you owe $0.05."
    assert alumni_reminder(alumni_new(1, "Di", 1)) == "Dear Di, you owe $0.00."


def test_hidden_thesis_advisor():
    assert thesis_new(1, "On Rivers", 2010, 1, Format.PRINT, "Dr. Lee").advisor == "Dr. Lee"
    assert thesis_line(thesis_new(7, "Atlas", 1999, 3, Format.AUDIO, "Kim")) == "Atlas (1999) [THE-7] 3/3 audio / Kim"


def test_hidden_thesis_plain():
    assert thesis_line(thesis_new(7, "Atlas", 1999, 3, Format.AUDIO, "")) == "Atlas (1999) [THE-7] 3/3 audio"
    assert [x.advisor for x in thesis_sample()] == ["", "", ""]
    assert thesis_available(thesis_checkout(thesis_new(1, "a", 2000, 2, Format.PRINT, "X"))) == 1


def test_hidden_staff_can_take():
    assert staff_can_take(staff_new(1, "a", 1), book_new(1, "b", 2000, 1, Format.PRINT)) == True
    assert staff_can_take(staff_new(1, "a", 1), book_checkout(book_new(1, "b", 2000, 1, Format.PRINT))) == False
    assert staff_can_take(staff_suspend(staff_new(1, "a", 1)), book_new(1, "b", 2000, 1, Format.PRINT)) == False
    assert staff_can_take(staff_charge(staff_new(1, "a", 1), 600), book_new(1, "b", 2000, 1, Format.PRINT)) == False


def test_hidden_unchanged():
    assert book_titles(book_sample()) == ["lost atlas", "northern voyage", "silent river"]
    assert fine_for(Format.PRINT, 10) == 250
    assert max_loans(3) == 10
