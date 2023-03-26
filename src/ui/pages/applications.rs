use std::cell::Ref;

use adw::{prelude::*, subclass::prelude::*};
use adw::{ResponseAppearance, Toast};
use gtk::glib::{self, clone, timeout_future_seconds, BoxedAnyObject, MainContext, Object};
use gtk::{gio, CustomSorter, FilterChange, Ordering, SortType};
use nix::sys::signal::Signal;

use crate::config::PROFILE;
use crate::i18n::{i18n, i18n_f};
use crate::ui::dialogs::app_dialog::ResAppDialog;
use crate::ui::widgets::application_name_cell::ResApplicationNameCell;
use crate::ui::window::MainWindow;
use crate::utils::processes::{App, Apps, SimpleItem};
use crate::utils::units::{to_largest_unit, Base};

mod imp {
    use std::cell::RefCell;

    use super::*;

    use gtk::CompositeTemplate;

    #[derive(Debug, CompositeTemplate, Default)]
    #[template(resource = "/me/nalux/Resources/ui/pages/applications.ui")]
    pub struct ResApplications {
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub search_revealer: TemplateChild<gtk::Revealer>,
        #[template_child]
        pub search_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        pub applications_scrolled_window: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub search_button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub information_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub end_application_button: TemplateChild<adw::SplitButton>,

        pub apps: RefCell<Apps>,
        pub store: RefCell<gio::ListStore>,
        pub selection_model: RefCell<gtk::SingleSelection>,
        pub filter_model: RefCell<gtk::FilterListModel>,
        pub sort_model: RefCell<gtk::SortListModel>,
        pub column_view: RefCell<gtk::ColumnView>,
        pub open_dialog: RefCell<Option<(String, ResAppDialog)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ResApplications {
        const NAME: &'static str = "ResApplications";
        type Type = super::ResApplications;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.install_action(
                "applications.kill-application",
                None,
                move |res_applications, _, _| {
                    res_applications
                        .execute_process_action_dialog_selected_app(Signal::SIGKILL);
                },
            );

            klass.install_action(
                "applications.halt-application",
                None,
                move |res_applications, _, _| {
                    res_applications
                        .execute_process_action_dialog_selected_app(Signal::SIGSTOP);
                },
            );

            klass.install_action(
                "applications.continue-application",
                None,
                move |res_applications, _, _| {
                    res_applications
                        .execute_process_action_dialog_selected_app(Signal::SIGCONT);
                },
            );

            Self::bind_template(klass);
        }

        // You must call `Widget`'s `init_template()` within `instance_init()`.
        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for ResApplications {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            // Devel Profile
            if PROFILE == "Devel" {
                obj.add_css_class("devel");
            }
        }
    }

    impl WidgetImpl for ResApplications {}
    impl BinImpl for ResApplications {}
}

glib::wrapper! {
    pub struct ResApplications(ObjectSubclass<imp::ResApplications>)
        @extends gtk::Widget, adw::Bin;
}

impl ResApplications {
    pub fn new() -> Self {
        glib::Object::new::<Self>()
    }

    pub fn init(&self) {
        self.setup_widgets();
        self.setup_signals();
        self.setup_listener();
    }

    pub fn setup_widgets(&self) {
        let imp = self.imp();

        let column_view = gtk::ColumnView::new(None::<gtk::SingleSelection>);
        let store = gio::ListStore::new(BoxedAnyObject::static_type());
        let filter_model = gtk::FilterListModel::new(
            Some(store.clone()),
            Some(gtk::CustomFilter::new(
                clone!(@strong self as this => move |obj| this.search_filter(obj)),
            )),
        );
        let sort_model = gtk::SortListModel::new(Some(filter_model.clone()), column_view.sorter());
        let selection_model = gtk::SingleSelection::new(Some(sort_model.clone()));
        column_view.set_model(Some(&selection_model));
        selection_model.set_can_unselect(true);
        selection_model.set_autoselect(false);

        *imp.selection_model.borrow_mut() = selection_model;
        *imp.sort_model.borrow_mut() = sort_model;
        *imp.filter_model.borrow_mut() = filter_model;
        *imp.store.borrow_mut() = store;

        let name_col_factory = gtk::SignalListItemFactory::new();
        let name_col =
            gtk::ColumnViewColumn::new(Some(&i18n("Application")), Some(name_col_factory.clone()));
        name_col.set_resizable(true);
        name_col.set_expand(true);
        name_col_factory.connect_setup(move |_factory, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let row = ResApplicationNameCell::new();
            item.set_child(Some(&row));
        });
        name_col_factory.connect_bind(move |_factory, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let child = item
                .child()
                .unwrap()
                .downcast::<ResApplicationNameCell>()
                .unwrap();
            let entry = item.item().unwrap().downcast::<BoxedAnyObject>().unwrap();
            let r: Ref<SimpleItem> = entry.borrow();
            child.set_name(&r.display_name);
            child.set_icon(Some(&r.icon));
        });
        let name_col_sorter = CustomSorter::new(move |a, b| {
            let item_a = a
                .downcast_ref::<BoxedAnyObject>()
                .unwrap()
                .borrow::<SimpleItem>();
            let item_b = b
                .downcast_ref::<BoxedAnyObject>()
                .unwrap()
                .borrow::<SimpleItem>();
            item_a.display_name.cmp(&item_b.display_name).into()
        });
        name_col.set_sorter(Some(&name_col_sorter));

        let memory_col_factory = gtk::SignalListItemFactory::new();
        let memory_col =
            gtk::ColumnViewColumn::new(Some(&i18n("Memory")), Some(memory_col_factory.clone()));
        memory_col.set_resizable(true);
        memory_col_factory.connect_setup(move |_factory, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let row = gtk::Inscription::new(None);
            item.set_child(Some(&row));
        });
        memory_col_factory.connect_bind(move |_factory, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let child = item
                .child()
                .unwrap()
                .downcast::<gtk::Inscription>()
                .unwrap();
            let entry = item.item().unwrap().downcast::<BoxedAnyObject>().unwrap();
            let r: Ref<SimpleItem> = entry.borrow();
            let (number, prefix) = to_largest_unit(r.memory_usage as f64, &Base::Decimal);
            child.set_text(Some(&format!("{number:.1} {prefix}B")));
        });
        let memory_col_sorter = CustomSorter::new(move |a, b| {
            let item_a = a
                .downcast_ref::<BoxedAnyObject>()
                .unwrap()
                .borrow::<SimpleItem>();
            let item_b = b
                .downcast_ref::<BoxedAnyObject>()
                .unwrap()
                .borrow::<SimpleItem>();
            item_a.memory_usage.cmp(&item_b.memory_usage).into()
        });
        memory_col.set_sorter(Some(&memory_col_sorter));

        let cpu_col_factory = gtk::SignalListItemFactory::new();
        let cpu_col =
            gtk::ColumnViewColumn::new(Some(&i18n("Processor")), Some(cpu_col_factory.clone()));
        cpu_col.set_resizable(true);
        cpu_col_factory.connect_setup(move |_factory, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let row = gtk::Inscription::new(None);
            item.set_child(Some(&row));
        });
        cpu_col_factory.connect_bind(move |_factory, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let child = item
                .child()
                .unwrap()
                .downcast::<gtk::Inscription>()
                .unwrap();
            let entry = item.item().unwrap().downcast::<BoxedAnyObject>().unwrap();
            let r: Ref<SimpleItem> = entry.borrow();
            child.set_text(Some(&format!("{:.1} %", r.cpu_time_ratio * 100.0)));
        });
        let cpu_col_sorter = CustomSorter::new(move |a, b| {
            let item_a = a
                .downcast_ref::<BoxedAnyObject>()
                .unwrap()
                .borrow::<SimpleItem>();
            let item_b = b
                .downcast_ref::<BoxedAnyObject>()
                .unwrap()
                .borrow::<SimpleItem>();
            // we have to do this because f32s do not implement `Ord`
            if item_a.cpu_time_ratio > item_b.cpu_time_ratio {
                Ordering::Larger
            } else if item_a.cpu_time_ratio < item_b.cpu_time_ratio {
                Ordering::Smaller
            } else {
                Ordering::Equal
            }
        });
        cpu_col.set_sorter(Some(&cpu_col_sorter));

        column_view.append_column(&name_col);
        column_view.append_column(&memory_col);
        column_view.append_column(&cpu_col);
        column_view.sort_by_column(Some(&name_col), SortType::Ascending);
        column_view.set_enable_rubberband(true);
        imp.applications_scrolled_window
            .set_child(Some(&column_view));
        *imp.column_view.borrow_mut() = column_view;

        *imp.apps.borrow_mut() = futures::executor::block_on(Apps::new()).unwrap();
        imp.apps
            .borrow()
            .simple()
            .iter()
            .map(|simple_item| BoxedAnyObject::new(simple_item.clone()))
            .for_each(|item_box| imp.store.borrow().append(&item_box));
    }

    pub fn setup_signals(&self) {
        let imp = self.imp();

        imp.selection_model.borrow().connect_selection_changed(
            clone!(@strong self as this => move |model, _, _| {
                let imp = this.imp();
                let is_system_processes = model.selected_item().map_or(false, |object| {
                    object
                    .downcast::<BoxedAnyObject>()
                    .unwrap()
                    .borrow::<SimpleItem>()
                    .clone()
                    .id
                    .is_none()
                });
                imp.information_button.set_sensitive(model.selected() != u32::MAX);
                imp.end_application_button.set_sensitive(model.selected() != u32::MAX && !is_system_processes);
            }),
        );

        imp.search_button
            .connect_toggled(clone!(@strong self as this => move |button| {
                let imp = this.imp();
                imp.search_revealer.set_reveal_child(button.is_active());
                if let Some(filter) = imp.filter_model.borrow().filter() {
                    filter.changed(FilterChange::Different)
                }
                if button.is_active() {
                    imp.search_entry.grab_focus();
                }
            }));

        imp.search_entry
            .connect_search_changed(clone!(@strong self as this => move |_| {
                let imp = this.imp();
                if let Some(filter) = imp.filter_model.borrow().filter() {
                    filter.changed(FilterChange::Different)
                }
            }));

        imp.information_button
            .connect_clicked(clone!(@strong self as this => move |_| {
                let imp = this.imp();
                let selection_option = imp.selection_model.borrow()
                .selected_item()
                .map(|object| {
                    object
                    .downcast::<BoxedAnyObject>()
                    .unwrap()
                    .borrow::<SimpleItem>()
                    .clone()
                });
                if let Some(selection) = selection_option {
                    let app_dialog = ResAppDialog::new();
                    app_dialog.init(&selection);
                    app_dialog.show();
                    *imp.open_dialog.borrow_mut() = Some((selection.id.unwrap_or_default(), app_dialog));
                }
            }));

        imp.end_application_button
            .connect_clicked(clone!(@strong self as this => move |_| {
                this.execute_process_action_dialog_selected_app(Signal::SIGTERM);
            }));
    }

    pub fn setup_listener(&self) {
        // TODO: don't use unwrap()
        let main_context = MainContext::default();
        main_context.spawn_local(clone!(@strong self as this => async move {
            loop {
                timeout_future_seconds(2).await;
                this.refresh_apps_list().await;
            }
        }));
    }

    fn search_filter(&self, obj: &Object) -> bool {
        let imp = self.imp();
        let item = obj
            .downcast_ref::<BoxedAnyObject>()
            .unwrap()
            .borrow::<SimpleItem>()
            .clone();
        let search_string = imp.search_entry.text().to_string().to_lowercase();
        !imp.search_revealer.reveals_child()
            || item.display_name.to_lowercase().contains(&search_string)
            || item
                .description
                .unwrap_or_default()
                .to_lowercase()
                .contains(&search_string)
    }

    fn get_selected_simple_item(&self) -> Option<SimpleItem> {
        self.imp()
            .selection_model
            .borrow()
            .selected_item()
            .map(|object| {
                object
                    .downcast::<BoxedAnyObject>()
                    .unwrap()
                    .borrow::<SimpleItem>()
                    .clone()
            })
    }

    async fn refresh_apps_list(&self) {
        let imp = self.imp();
        let selection = imp.selection_model.borrow();
        let mut apps = imp.apps.borrow_mut();

        // if we reuse the old ListStore, for some reason setting the
        // vadjustment later just doesn't work most of the time.
        // so we just make a new one every refresh instead :')
        // TODO: make this less hacky
        let new_store = gio::ListStore::new(BoxedAnyObject::static_type());

        // this might be very hacky, but remember the ID of the currently
        // selected item, clear the list model and repopulate it with the
        // refreshed apps and stats, then reselect the remembered app.
        // TODO: make this even less hacky
        let selected_item = self
            .get_selected_simple_item()
            .map(|simple_item| simple_item.id);
        if apps.refresh().await.is_ok() {
            apps.simple()
                .iter()
                .map(|simple_item| {
                    if let Some((id, dialog)) = &*imp.open_dialog.borrow() && simple_item.id.clone().unwrap_or_default().as_str() == id.as_str() {
                        dialog.set_cpu_usage(simple_item.cpu_time_ratio);
                        dialog.set_memory_usage(simple_item.memory_usage);
                        dialog.set_processes_amount(simple_item.processes_amount);
                    }
                    BoxedAnyObject::new(simple_item.clone())
                })
                .for_each(|item_box| new_store.append(&item_box));
        }
        imp.filter_model.borrow().set_model(Some(&new_store));
        *imp.store.borrow_mut() = new_store;

        // find the (potentially) new index of the process that was selected
        // before the refresh and set our selection to that index
        if let Some(selected_item) = selected_item {
            let new_index = selection
                .iter::<glib::Object>()
                .position(|object| {
                    object
                        .unwrap()
                        .downcast::<BoxedAnyObject>()
                        .unwrap()
                        .borrow::<SimpleItem>()
                        .id
                        == selected_item
                })
                .map(|index| index as u32);
            if let Some(index) = new_index && index != u32::MAX {
                selection.set_selected(index);
            }
        }
    }

    fn get_selected_app(&self) -> Option<App> {
        let apps = &self.imp().apps.borrow().apps;

        self.get_selected_simple_item()
            .and_then(|simple_item| simple_item.id)
            .and_then(|id| apps.get(&id).cloned())
    }

    fn execute_process_action(&self, app: &App, signal: Signal) {
        let res = app.signal(signal);

        let processes_tried = res.len();
        let processes_successful = res.iter().flatten().count();
        let processes_unsuccessful = processes_tried - processes_successful;

        #[rustfmt::skip]
        let toast_message = match processes_unsuccessful {
            0 => i18n_f("{} {}", &[get_action_success(signal), &app.display_name()]),
            1 => i18n(get_action_failure(signal)),
            _ => i18n_f("{} {} processes", &[get_action_failure_multiple(signal), &processes_unsuccessful.to_string()]),
        };

        self.imp()
            .toast_overlay
            .add_toast(Toast::new(&toast_message));
    }

    pub fn execute_process_action_dialog(&self, app: &App, signal: Signal) {
        // Nothing too bad can happen on Continue so dont show the dialog
        if signal == Signal::SIGCONT {
            self.execute_process_action(&app, signal);
            return;
        }

        // Confirmation dialog & warning
        let dialog = adw::MessageDialog::builder()
            .transient_for(&MainWindow::default())
            .modal(true)
            .heading(i18n_f(
                "{} {}?",
                &[get_action_name(signal), &app.display_name()],
            ))
            .body(i18n(get_action_warning(signal)))
            .build();

        dialog.add_response("yes", &i18n(get_action_description(signal)));
        dialog.set_response_appearance("yes", ResponseAppearance::Destructive);

        dialog.add_response("no", &i18n("Cancel"));
        dialog.set_default_response(Some("no"));
        dialog.set_close_response("no");

        // Called when "yes" or "no" were clicked
        dialog.connect_response(
            None,
            clone!(@strong self as this, @strong app => move |_, response| {
                if response == "yes" {
                    this.execute_process_action(&app, signal);
                }
            }),
        );

        dialog.show();
    }

    pub fn execute_process_action_dialog_selected_app(&self, signal: Signal) {
        if let Some(app) = self.get_selected_app() {
            self.execute_process_action_dialog(&app, signal);
        }
    }
}

const fn get_action_name(signal: Signal) -> &'static str {
    match signal {
        Signal::SIGTERM => "End",
        Signal::SIGSTOP => "Halt",
        Signal::SIGKILL => "Kill",
        Signal::SIGCONT => "Continue",
        _ => panic!("Unsupported signal"),
    }
}

const fn get_action_warning(signal: Signal) -> &'static str {
    match signal {
            Signal::SIGTERM  => "Unsaved work might be lost.",
            Signal::SIGSTOP => "Halting an application can come with serious risks such as losing data and security implications. Use with caution.",
            Signal::SIGKILL => "Killing an application can come with serious risks such as losing data and security implications. Use with caution.",
            Signal::SIGCONT => "",
            _ => panic!("Unsupported signal"),
        }
}

const fn get_action_description(signal: Signal) -> &'static str {
    match signal {
        Signal::SIGTERM => "End application",
        Signal::SIGSTOP => "Halt application",
        Signal::SIGKILL => "Kill application",
        Signal::SIGCONT => "Continue application",
        _ => panic!("Unsupported signal"),
    }
}

const fn get_action_success(signal: Signal) -> &'static str {
    match signal {
        Signal::SIGTERM => "Successfully ended",
        Signal::SIGSTOP => "Successfully halted",
        Signal::SIGKILL => "Successfully killed",
        Signal::SIGCONT => "Successfully continued",
        _ => panic!("Unsupported signal"),
    }
}

const fn get_action_failure(signal: Signal) -> &'static str {
    match signal {
        Signal::SIGTERM => "There was a problem ending a process",
        Signal::SIGSTOP => "There was a problem halting a process",
        Signal::SIGKILL => "There was a problem killing a process",
        Signal::SIGCONT => "There was a problem continuing a process",
        _ => panic!("Unsupported signal"),
    }
}

const fn get_action_failure_multiple(signal: Signal) -> &'static str {
    match signal {
        Signal::SIGTERM => "There were problems ending",
        Signal::SIGSTOP => "There were problems halting",
        Signal::SIGKILL => "There were problems killing",
        Signal::SIGCONT => "There were problems continuing",
        _ => panic!("Unsupported signal"),
    }
}
