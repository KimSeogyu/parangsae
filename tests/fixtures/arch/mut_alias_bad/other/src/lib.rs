use owner::Ledger as Book;

pub fn poke(book: &mut Book) {
    book.cash = 4;
}
