use anyhow::Error;
use rtw::{
    Activity, ActivityId, ActivityService, CurrentActivityRepository, DateTimeW,
    FinishedActivityRepository, OngoingActivity,
};

pub struct Service<F, C>
where
    F: FinishedActivityRepository,
    C: CurrentActivityRepository,
{
    finished: F,
    current: C,
}

impl<F, C> Service<F, C>
where
    F: FinishedActivityRepository,
    C: CurrentActivityRepository,
{
    pub fn new(finished: F, current: C) -> Self {
        Service { finished, current }
    }
}

impl<F, C> ActivityService for Service<F, C>
where
    F: FinishedActivityRepository,
    C: CurrentActivityRepository,
{
    fn get_current_activity(&self) -> anyhow::Result<Option<OngoingActivity>> {
        self.current.get_current_activity()
    }

    fn start_activity(&mut self, activity: OngoingActivity) -> anyhow::Result<OngoingActivity> {
        self.stop_current_activity(activity.start_time)?;
        let started = OngoingActivity::new(activity.start_time, activity.tags);
        self.current.set_current_activity(started.clone())?;
        Ok(started)
    }

    fn stop_current_activity(&mut self, time: DateTimeW) -> anyhow::Result<Option<Activity>> {
        let current = self.current.get_current_activity()?;
        match current {
            None => Ok(None),
            Some(current_activity) => {
                self.finished
                    .write_activity(current_activity.clone().into_activity(time)?)?;
                self.current.reset_current_activity()?;
                Ok(Some(current_activity.into_activity(time)?))
            }
        }
    }

    fn filter_activities<P>(&self, p: P) -> Result<Vec<(ActivityId, Activity)>, Error>
    where
        P: Fn(&(ActivityId, Activity)) -> bool,
    {
        self.finished.filter_activities(p)
    }

    fn delete_activity(&self, id: ActivityId) -> Result<Option<Activity>, Error> {
        self.finished.delete_activity(id)
    }

    fn track_activity(&mut self, activity: Activity) -> Result<Activity, Error> {
        self.finished.write_activity(activity.clone())?;
        Ok(activity)
    }
}

#[cfg(test)]
mod tests {
    use crate::chrono_clock::ChronoClock;
    use crate::json_current::JsonCurrentActivityRepository;
    use crate::json_finished::JsonFinishedActivityRepository;
    use crate::service::Service;
    use rtw::{ActivityService, Clock, DateTimeW, OngoingActivity};
    use tempfile::{tempdir, TempDir};

    fn build_json_service(
        test_dir: &TempDir,
    ) -> Service<JsonFinishedActivityRepository, JsonCurrentActivityRepository> {
        let writer_path = test_dir.path().join(".rtww.json");
        let repository_path = test_dir.path().join(".rtwr.json");
        Service::new(
            JsonFinishedActivityRepository::new(writer_path),
            JsonCurrentActivityRepository::new(repository_path),
        )
    }

    #[test]
    fn test_no_activity() {
        let clock = ChronoClock {};
        let test_dir = tempdir().expect("error while creating tempdir");
        let mut service = build_json_service(&test_dir);
        assert!(service.stop_current_activity(clock.get_time()).is_ok());
        assert!(service.get_current_activity().unwrap().is_none());
    }

    #[test]
    fn test_start_activity() {
        let clock = ChronoClock {};
        let test_dir = tempdir().expect("error while creating tempdir");
        let mut service = build_json_service(&test_dir);
        assert!(service.stop_current_activity(clock.get_time()).is_ok());
        let start = service.start_activity(OngoingActivity {
            start_time: clock.get_time(),
            tags: vec![String::from("a")],
        });
        start.unwrap();
        let current = service.get_current_activity();
        assert!(current.is_ok());
        assert!(current.unwrap().is_some());
    }

    #[test]
    fn test_stop_activity_with_active() {
        let clock = ChronoClock {};
        let test_dir = tempdir().expect("error while creating tempdir");
        let mut service = build_json_service(&test_dir);
        let start = service.start_activity(OngoingActivity {
            start_time: clock.get_time(),
            tags: vec![String::from("a")],
        });
        start.unwrap();
        assert!(service.get_current_activity().unwrap().is_some());
        assert!(service.stop_current_activity(clock.get_time()).is_ok());
        assert!(service.get_current_activity().unwrap().is_none());
    }

    #[test]
    fn test_start_stop_start() {
        let clock = ChronoClock {};
        let test_dir = tempdir().expect("error while creating tempdir");
        let mut service = build_json_service(&test_dir);
        let start_0 = service.start_activity(OngoingActivity {
            start_time: clock.get_time(),
            tags: vec![String::from("a")],
        });
        assert!(start_0.is_ok());
        assert!(service.get_current_activity().unwrap().is_some());
        let stop = service.stop_current_activity(clock.get_time());
        assert!(stop.is_ok());
        assert!(service.get_current_activity().unwrap().is_none());
        let start_1 = service.start_activity(OngoingActivity {
            start_time: clock.get_time(),
            tags: vec![String::from("b")],
        });
        assert!(start_1.is_ok());
        assert!(service.get_current_activity().unwrap().is_some());
    }

    #[test]
    fn test_summary_nothing() {
        let clock = ChronoClock {};
        let test_dir = tempdir().expect("error while creating tempdir");
        let service = build_json_service(&test_dir);
        let (today_start, today_end) = clock.today_range();
        let activities = service.filter_activities(|(_id, a)| {
            today_start <= a.get_start_time() && a.get_start_time() <= today_end
        });
        assert!(activities.is_ok());
    }

    #[test]
    fn test_summary_something() {
        let test_dir = tempdir().expect("error while creating tempdir");
        let mut service = build_json_service(&test_dir);
        let today = chrono::Local::today();
        let range_start: DateTimeW = today.and_hms(8, 0, 0).into();
        let activity_start: DateTimeW = today.and_hms(8, 30, 0).into();
        let activity_end: DateTimeW = today.and_hms(8, 45, 0).into();
        let range_end: DateTimeW = today.and_hms(9, 0, 0).into();
        let _start = service.start_activity(OngoingActivity::new(
            activity_start,
            vec![String::from("a")],
        ));
        let _stop = service.stop_current_activity(activity_end);
        let activities = service.filter_activities(|(_id, a)| {
            range_start <= a.get_start_time() && a.get_start_time() <= range_end
        });
        assert!(!activities.unwrap().is_empty());
    }

    #[test]
    fn test_summary_not_in_range() {
        let test_dir = tempdir().expect("error while creating tempdir");
        let mut service = build_json_service(&test_dir);
        let today = chrono::Local::today();
        let range_start: DateTimeW = today.and_hms(9, 0, 0).into();
        let activity_start: DateTimeW = today.and_hms(8, 30, 0).into();
        let activity_end: DateTimeW = today.and_hms(8, 45, 0).into();
        let range_end: DateTimeW = today.and_hms(10, 0, 0).into();
        let _start = service.start_activity(OngoingActivity::new(
            activity_start,
            vec![String::from("a")],
        ));
        let _stop = service.stop_current_activity(activity_end);
        let activities = service.filter_activities(|(_id, a)| {
            range_start <= a.get_start_time() && a.get_start_time() <= range_end
        });
        assert!(activities.unwrap().is_empty());
    }
}
